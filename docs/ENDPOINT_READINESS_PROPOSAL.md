# Managed-listener and application-probed endpoint readiness

Status: design for review, 29 September 2026. The macOS FD-only and Reth
protocol-probe feasibility checks passed on this host with the pinned Reth
package. Production implementation, failure-path proof, and the exact ABI
cutover remain open. This is not the shipped contract. Until a complete
cutover is implemented and verified, [`CONTRACT.md`](CONTRACT.md) remains
normative and endpoint starts that return `PORT_UNVERIFIABLE` still refuse.

## Decision to develop

Replace PORT-1's required **negative host-wide listener inventory** with a
**positive observation of a listener held by the managed service**, plus a
required application probe for each declared endpoint. Keep one revised
guarantee on Linux and macOS. This retains a kernel-backed connection between
the declared endpoint and the service process while avoiding a dependency on
unproven privileged host-wide absence evidence.

Proposed readiness invariant: one bounded **service-wide check round** must
succeed before ready commit. In that round, for every declared endpoint:

1. The service's recorded process is live and contained. A process in that
   service's verified containment holds an inspectable TCP socket in `LISTEN`
   state at the planned **exact** address, family, and port. A wildcard-only
   socket does not satisfy the declaration.
2. The application-defined ready probe attached to that endpoint succeeds in
   this same round. Its invocation renders bare `${host}` and `${port}` from
   that endpoint. The author or adapter must make it check the assigned
   address and meaningful protocol behavior; the runtime can prove its exit
   status and attachment, not the meaning of arbitrary command bytes.
3. After all probes, the runtime rechecks process identity, containment, and
   the **same** listener witnesses. It then atomically records the complete
   endpoint set and ready transition. Failure or uncertainty cannot commit
   readiness or start dependent work.

Health repeats the same whole-set round at the existing health phase. It does
not introduce a continuous health daemon.
Endpoint-less services retain invocation-probed readiness without an
addressability claim.

The revised invariant does **not** claim that the host has no conflicting
listener, that the observed socket is exclusively held by the service, or that
the observed process answered the protocol probe. These are separate facts.
An old or unrelated process can share a socket or answer a probe whose command
does not establish instance identity. A per-run application challenge or a
runtime-bound socket handed to a cooperating child would be needed for a
stronger responder claim; neither is generic to today's service invocation.
No output or event may present probe success as exclusive socket ownership.
The OS observations establish facts at their observation points, not a
continuous or OS-atomic reservation across the protocol requests and commit.

## Evidence behind the boundary

[`PROBLEM_MACOS.md`](../PROBLEM_MACOS.md) records an unprivileged
`net.inet.tcp.pcblist_n` body that omitted a controlled child listener, as
well as privileged positive sightings that did not certify a complete negative
inventory. A process FD scan is not a negative listener inventory either: a
Mach fileport and queued Unix `SCM_RIGHTS` can keep a listener alive without an
ordinary socket FD. The recorded wildcard/exact bind experiment also shows
why an exact `SO_REUSEADDR` bind can succeed despite a wildcard listener.

Later unprivileged runs on the same macOS build returned substantially more
PCB records and correlated the child FD, including a sample with 70 advertised
and 69 decoded records. Visibility can therefore vary; a successful sample or
matching count cannot establish a durable host-wide absence rule. The useful
narrower fact is that the known child's socket FD was inspectable without sudo
in these runs. Apple's [`socket_fdinfo` and `tcp_sockinfo` definitions](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/proc_info.h)
carry local address, port, TCP state, and socket identity. The corresponding
[`libproc` interfaces](https://github.com/apple-oss-distributions/xnu/blob/main/libsyscall/wrappers/libproc/libproc.h)
are private and subject to change; unsupported layouts or denied reads must be
reported as uncertainty, never as an empty socket set.

The original diagnostic correlates a known child FD to a PCB socket handle;
the new controlled FD-only check below closes its main feasibility gap. It is
still a prototype on one host, not proof that the production observer rejects
every denied, malformed, or stale observation correctly.

## Feasibility checks completed

On Darwin 27.0.0 arm64, `cc -Wall -Wextra -Werror` built
[`prove_macos_fd_listener.c`](../runtime/crates/nixfied-runtime/tests/fixtures/prove_macos_fd_listener.c).
It ran without sudo or host PCB reads. `proc_pidinfo(PROC_PIDLISTFDS)` and
`proc_pidfdinfo(PROC_PIDFDSOCKETINFO)` supplied full `socket_fdinfo` records
for a child and a reparented member of its process group. The check decoded
exact IPv4 and IPv6 loopback tuples and TCP `LISTEN` for IPv6 listeners
configured with either `IPV6_V6ONLY` value. It also checked socket identity
and generation; it rejected a wildcard listener as an exact one, and rejected
a bound socket that had not called `listen`. Closing and rebinding the
same IPv4 tuple changed socket identity. `proc_listpgrppids` found the
reparented member, and its FD was inspectable. Four direct-child runs and three
reparented-member runs passed. The test uses the installed SDK structures and
requires the complete returned record length, avoiding guessed field offsets.

The same host's existing
[`prove_observer_feasibility.py`](../tests/macos/prove_observer_feasibility.py)
then reproduced the negative-inventory defect: five unprivileged samples
advertised 64–65 PCB records but decoded only one, omitted its known child
listener, and still read that child's socket FD. The C check's success does
not depend on a PCB body at all. This establishes the proposed **positive
FD-only path's feasibility on this host**, including a host context with the
known PCB omission. The two fixtures used separate controlled children, so
they do not supply a simultaneous same-socket PCB/FD comparison. They do not
certify other macOS builds or host policies.

The pinned `reth` package evaluated to version 1.9.3. The standalone
[`prove-reth-endpoint-probes.py`](../nix/checks/prove-reth-endpoint-probes.py)
started that binary with the adapter's three loopback listener flags and a
temporary JWT file. It received a valid `eth_blockNumber` result over HTTP
and a real WebSocket upgrade and masked JSON-RPC exchange; a JWT-signed
`engine_exchangeCapabilities` call returned 17 method names. Missing and
incorrect JWTs both returned HTTP 401 in repeated runs. A nonexistent method
returned **HTTP 200 with a JSON-RPC error**, which the check rejected. The
current adapter's `curl -sf` exit alone would accept that last response, so
the adapter cutover must validate JSON-RPC bodies, not only transport status.
These methods follow
the [Reth JSON-RPC transport guide](https://reth.rs/jsonrpc/intro/) and the
[Engine API capabilities](https://github.com/ethereum/execution-apis/blob/main/src/engine/common.md)
and [JWT authentication](https://github.com/ethereum/execution-apis/blob/main/src/engine/authentication.md)
specifications.

## Owners and rejection boundaries

| Fact or invariant | Owner | Rejection boundary |
| --- | --- | --- |
| Each endpoint has one ready and one health application invocation | Nix declarations/compiler and independent Rust manifest validation | Reject absent, extra, misplaced, or incoherent probes before prepare, state mutation, or spawn |
| Planned address and endpoint set | Existing Nix derivation and Rust admission/selection | Reject invalid references, port plans, and declaration/plan mismatches before effects |
| Startup coordination and exact-bind availability | Rust endpoint owner | Reject lock contention or unavailable bind before service prepare; bind success is not absence proof |
| Live process, containment, and exact listener witness | Rust runtime against OS process/socket facts | Reject unprovable inspection before ready commit; wait for an absent witness only when inspection is complete for the relevant managed processes |
| One retry policy and one whole-set round per lifecycle phase; probe execution, cancellation, capture, and settlement | Existing Rust lifecycle owner | Failed or unprovable round cannot advance readiness or health |
| Protocol meaning and target use within an opaque invocation | Service author or adapter | Adapter tests prove concrete commands; runtime validates attachment and observes exit, but cannot infer command semantics |
| Durable endpoint evidence and ready status | Existing transactional registry owner | Commit the exact endpoint set and ready status together, or neither |

The accepted listener witness is **positive**. The runtime need not enumerate
unrelated processes or establish an empty host-wide PCB stream. It must not
turn a failed inspection of a candidate managed process into an empty FD list.
If a service has no accepted witness after a complete relevant inspection,
readiness remains pending until its deadline. If inspection is denied,
malformed, unsupported, or cannot establish the process identity and
containment, readiness is unprovable rather than merely slow.

## Proposed authoring and wire contract

Keep the existing service endpoint forms. Add a required invocation-valued
`readyProbe` and `healthProbe` to the single `endpoint` shorthand and to each
member of `endpoints`. Normalize both forms to one manifest `Endpoint` record:
`{ endpointId, host, readyProbe: InvocationSpec, healthProbe: InvocationSpec }`.
Attachment supplies the endpoint identity without another probe-ID map. Nix
must require the invocation at authoring time; neither an implicit TCP probe
nor an empty invocation is a valid default.

Replace the old `ProbeSpec` and `ProbeKind` wire types with a `ProbePolicy`
record: `{ timeoutMs: NonZeroU64, retryIntervalMs: NonZeroU64,
maxAttempts: NonZeroU32 }`. `ReadySpec` and `HealthSpec` each carry their
existing `operationId` and `terminal`, a **required** `policy: ProbePolicy`,
and an optional `probe: InvocationSpec`. The scalar `probe` is present exactly
when the service is endpoint-less. Endpoint-bearing services must omit it and
provide both invocations on every endpoint. This cross-field alternative is
validated independently by the Nix compiler and raw Rust manifest admission;
old wire records with `ProbeSpec.kind` reject. Avoid a new manifest version
merely to alias old bytes: update the authored inventory and runtime ABI digest
as part of the exact cutover.

Authoring mirrors the wire shape: `lifecycle.ready.policy` and
`lifecycle.health.policy` own the per-phase retry budget, while
`lifecycle.ready.probe` and `lifecycle.health.probe` are only for endpoint-less
services. The existing numeric policy defaults may be retained at the Nix
authoring layer, then serialized explicitly. An endpoint-less service still
requires one invocation for each phase. Its start closure retains the existing
non-listener attestation rule. No service kind, daemon, or new semantic seam is
needed.

Within an endpoint-attached invocation, bare `${host}` and `${port}` resolve
to that attached endpoint, including when it is not primary. Explicit named
endpoint and dependency references keep their existing scope rules. Within
an endpoint-less probe, bare endpoint placeholders remain invalid. Validate
all references and closure bindings at Nix compilation and Rust admission;
the placeholder substitution must use the selected runtime port, never an
authored or guessed port. The runtime cannot infer whether opaque executable
arguments truly target that address, so adapters must prove that behavior.

Lower the validated wire into a closed service-addressing type, conceptually:

```text
ServiceAddressing =
    Endpointless { ready_probe: Invocation, health_probe: Invocation }
  | Endpoints {
      primary: EndpointId,
      entries: NonEmptyMap<EndpointId, EndpointPlan {
        host: LoopbackHost,
        ready_probe: Invocation,
        health_probe: Invocation
      }>
    }
ReadyOp  = { operation: OperationMeta, policy: ProbePolicy }
HealthOp = { operation: OperationMeta, policy: ProbePolicy }
```

`EndpointId`, `LoopbackHost`, and the positive policy fields should be
validated types with private construction. Admission checks the nonempty set,
the primary member, unique operation IDs, invocation/closure references, and
the exact endpoint-to-port plan before producing this type. Selection binds
each `EndpointPlan` to its assigned address and port once. This removes the
runtime combination of an endpoint map, a nullable primary, and an unrelated
scalar `Probe::Tcp | Probe::Exec`; a later phase cannot select a probe without
its endpoint or silently fall back to TCP connect.

The round runner should return a private `CompletedRound` only after every
check has passed. Its endpoint-bearing variant contains the exact selected
endpoint key set, paired final `ListenerWitness` values, and successful
probe-process references; its endpoint-less variant contains the successful
scalar probe reference. A private constructor compares initial and final
witness identities and checks the key set. The registry accepts this value,
so partial successes and a witness set from another round are not expressible
at the commit API.

`timeoutMs` bounds **each invocation attempt**, `maxAttempts` counts complete
service-wide rounds, and `retryIntervalMs` is the delay between rounds. A
failed, timed-out, or unstarted probe never carries a success credit into a
later round. The operation can take roughly `maxAttempts` times the sum of
per-endpoint command deadlines plus inter-round delays, with OS, capture, and
cleanup overhead; it is not a strict wall-clock guarantee. The invocation's
own `timeoutMs` does not override the phase policy for probe attempts.

## Startup and observer contract

Keep the fixed host startup lock from preflight through ready commit or failed
startup cleanup. Attempt an exact bind with `SO_REUSEADDR` for each endpoint so
compatible `TIME_WAIT` restarts remain possible. `EADDRINUSE` means
preparation-time `PORT_CONFLICT` with a new reason such as
`bind-unavailable`; it does not identify a foreign listener. Other bind or
inspection errors retain typed uncertainty. The preflight socket closes
before the child starts, so an unrelated host process can race. A successful
bind establishes availability at that moment only; it does not prove host-wide
absence or exclude wildcard coexistence. Remove the current mandatory
negative listener inventory, including its `pcblist_n` and Linux equivalents.

The existing process/containment owner supplies fresh candidate PIDs: the
service leader plus eligible members of its required process group or process
tree. Process-group membership must be enumerated as a group, not inferred
solely from descendant history; process-tree membership uses refreshed
descendants. Verify PID start identity and current containment before and
after inspecting each candidate. The endpoint observer inspects only those
managed candidates' relevant socket FDs. It does not establish that unrelated
host processes hold no matching socket.

The observer returns a closed outcome, conceptually
`Complete(CompleteWitnessSet) | Pending(endpoint, reason) |
Unverifiable(reason)`. `CompleteWitnessSet` has a private constructor and
exactly one positive witness per selected endpoint; it cannot be made from
planned coordinates or probe exit status. A `ListenerWitness` binds the
selected exact address/family/port to the verified holder PID/start identity,
containment, and stable socket identity/generation. `Pending` means a complete
relevant inspection found no exact managed listener, or ordinary candidate FD
churn prevents a stable sighting. `Unverifiable` means denied inspection,
unsupported or malformed records, or unbounded list growth. The containment
owner's inability to establish process identity or membership remains a
separate `PROC_ESCAPE` failure, before a complete witness can be constructed.
Once a valid positive witness is found, unreadable unrelated candidate FDs
need not defeat it; there is no exclusivity or negative completeness claim.
If no witness is found and a candidate FD could not be inspected for a reason
other than ordinary churn,
the result is `Unverifiable`, not `Pending`. A missing witness must never be
inferred from an empty result fabricated after an OS inspection error.

On macOS, use `proc_pidinfo(PROC_PIDLISTFDS)` and
`proc_pidfdinfo(PROC_PIDFDSOCKETINFO)` for candidate PIDs. The decoder must
prove the exact local IPv4 or IPv6 tuple, `LISTEN` state, and a stable socket
identity from complete, layout-checked records without relying on the PCB
body. On Linux, correlate a candidate process FD's socket inode/cookie with
the kernel's TCP listening socket data (the existing `SOCK_DIAG` route can be
narrowed to positive matching). Neither platform may make an unrelated host
FD inventory a condition for success. Treat disappearing PIDs/FDs as ordinary
churn only after a valid process observation; denied, malformed, or unsupported
inspection remains an error. A listener retained solely by a fileport or
queued socket right, with no inspectable managed listening FD, is outside the
initial supported set.

The production macOS FD decoder remains a release gate. It must reproduce
the prototype's positive result and fail closed under supported host policies.
If it cannot establish the tuple, listen state, and socket identity, do not
substitute the application probe or a privileged PCB scan as an implicit
weaker fallback. Return to design review for a cooperating listener handoff,
a different positive witness, or an explicit support limit.

## Whole-service round and commit

The lifecycle owner runs a phase using this order for each attempt:

1. Check cancellation, service leader identity/liveness, containment, and
   the existing startup/checkpoint obligations. Obtain a complete exact
   managed-listener witness set for endpoint-bearing services. An endpoint-less
   service has no listener observation.
2. Run endpoint invocations in canonical endpoint-ID order, or the one
   endpoint-less invocation. Apply the phase policy timeout to each. Check
   cancellation, liveness, and containment during the wait and between probes.
   Stop the round at the first nonzero exit, timeout, or pending witness.
   Preserve each attempt's separate captured output and process evidence.
3. After **all** invocations succeed, observe the entire listener set again.
   Require the same endpoint keys, holder process start identities, and socket
   identities/generations as step 1. Search specifically for each initial
   witness, so a newly enumerated second listener cannot hide a surviving
   first one or substitute for a lost one. Recheck cancellation, liveness, and
   containment immediately before the registry transition. A changed or
   missing witness invalidates the whole round; no earlier probe success is
   reused on retry. Endpoint-less service only performs the final process
   checks.
4. On ready, pass the complete set and successful probe-attempt references to
   one registry transaction that validates the immutable process-bound endpoint
   rows and records endpoint evidence, ready status, and lifecycle terminal
   success together. A registry failure is terminal, not a probe retry. On
   health, use the same round rules and record the health terminal and endpoint
   evidence in its owning transaction; health does not re-mark ready or create
   another status authority.

`Pending`, nonzero exit, timeout, and witness replacement consume a round and
may retry. Exhausted rounds yield `READINESS_TIMEOUT` for readiness, with a
redaction-safe endpoint ID and last reason. Health exhaustion retains the
existing `READINESS_TIMEOUT` code and health lifecycle class. `Unverifiable` yields
`PORT_UNVERIFIABLE` immediately for an endpoint-bearing phase. Cancellation,
process exit or identity change, containment escape, capture/settlement
failure, and registry corruption keep their existing typed phase priorities;
they must not be turned into a probe retry. The process and containment checks
must also occur while a long probe waits, not only at round boundaries.
Initial health failure after ready retains the current
failed-service settlement behavior.

The checks narrow races but do not make the OS observations and protocol
requests one atomic operation. The success claim is limited to the observed
round, the recorded witnesses, and completed invocation exits. No event or
display should call the listener *exclusively owned* or attribute the probe
response to the witness holder.

## Reth adapter cutover

Give Reth one adapter-owned, short-lived protocol-check helper with three
closed modes. Package it as a declared closure and attach a mode-specific
invocation to each endpoint's `readyProbe` and `healthProbe`. Pass only the
mode, attached `${host}`, attached `${port}`, and, for authenticated RPC, the
`${stateDir}` path. The helper must use the planned port supplied by the
runtime and exit nonzero on transport, JSON parse, JSON-RPC `error`, missing
`result`, wrong response ID, or wrong result shape. The standalone feasibility
script demonstrates the required exchanges with Python's standard library;
the production helper can reuse those routines without putting domain logic
in Rust.

| Endpoint | Required exchange | Accepted result |
| --- | --- | --- |
| `reth-http` | HTTP POST `eth_blockNumber` | JSON-RPC 2.0 response with matching ID and a hex quantity result |
| `reth-ws` | RFC 6455 upgrade, then a masked text-frame `eth_blockNumber` request | Valid upgrade and JSON-RPC 2.0 response with matching ID and a hex quantity result |
| `reth-authrpc` | HTTP POST `engine_exchangeCapabilities` with `params: [[]]` and a fresh HS256 JWT | JSON-RPC 2.0 response with matching ID and a nonempty string array of Engine API methods |

Read the existing `${stateDir}/reth/config/jwt.hex` file that the adapter's
start wrapper supplies to Reth. Generate a short-lived JWT with an `iat` claim
inside the helper. Keep the secret and token out of the manifest, invocation
arguments/environment, runtime command JSON, and printed output; only the
state path crosses the invocation boundary. The helper can make the HTTP
request directly, so it need not pass the bearer token as a child process
argument. The check does not mutate chain state or require an instance
challenge. Authenticated Engine API availability is the claim, not response
identity.

The current Reth phase policy (`120` attempts, `2000` ms per probe, `500` ms
between attempts) was sized for one scalar probe. With three serial probes,
its conservative command-time upper estimate exceeds the runtime gate's
60-second task timeout. Choose phase budgets and gate timeout together during
the cutover, then prove cold startup and a bounded failure on both platforms.
Do not silently keep the old attempt count while multiplying per-round work.

## Evidence and ABI cutover

Process-bound endpoint rows continue to record planned coordinates and the
service process association; they are not listener ownership leases. Recommend
one new event vocabulary, `endpoint.check-succeeded`, emitted once per
endpoint on a successful ready or health round. Its redaction-safe payload has
`phase` (`ready` or `health`), `endpointId`, the selected `address` and `port`,
the positive `listener` witness (`holderPid`, process-start identity, socket
identity and generation where available), and the successful probe attempt's
existing process key. This says **which socket was observed** and **which
command exited successfully**, without asserting they are the same responder.
Do not put executable arguments, secrets, or captured output in this event.
The exact platform witness encoding is part of the ABI review: it must be
stable enough to compare within a run without pretending macOS and Linux have
the same kernel identifiers. The event is evidence at observation time, not
durable ownership status.

At ready commit, the registry compares the complete selected endpoint set to
the immutable rows, then writes all `endpoint.check-succeeded` events, the
service ready status, the existing `service.probe-ready` transition, and ready
lifecycle success in **one** transaction. Retain `service.probe-ready` only
with its existing service-level meaning; it supplies no endpoint or responder
claim. For endpoint-less services, write no endpoint events. On health success,
write the health endpoint events and health lifecycle success in one
transaction, without changing ready status. A failed round writes no success
events; ordinary attempt process and capture records remain available for
diagnosis. The event writer takes a checked complete round result rather than
parallel uncorrelated arrays of witnesses and probes.

Do not emit new `port.owner-verified` events under the revised meaning, and do
not reinterpret or rewrite historical events. Readers and summaries must
distinguish historical ownership evidence from the new observed-check evidence.
Do not add a listener-state table, runtime cache, or second mutable authority.

`PORT_UNVERIFIABLE` remains appropriate for denied or incoherent required
managed-process/socket inspection. An absent exact witness after a complete
relevant inspection, or failed application probes, ends as a readiness failure
after the configured attempts. Lock contention and unavailable bind remain
preparation-time refusals. The existing `PORT_CONFLICT` reason vocabulary
asserts a listener in cases where the new path may know only that bind failed;
add `bind-unavailable` and revise output semantics rather than mislabeling that
fact. Reserve `listener-occupied` for historical events or a future path that
actually proves that claim; never infer it from `EADDRINUSE` alone.
Preserve existing cancellation, process escape, capture, and settlement
priorities when placing the new observation in the lifecycle.

This is one contract/ABI cutover, not a macOS-only fallback. Revise
[`CONTRACT.md`](CONTRACT.md), architecture, the authored
[`capability.txt`](../runtime/crates/nixfied-manifest/capability.txt), Nix/Rust
declarations and validators, ABI snapshot, fixtures, output readers, adapter
guidance, and gates together. The main code sites are `nix/modules/` and
`nix/compiler/` for authoring, derivation, and validation;
`nix/meta/manifest.nix` and `nixfied-manifest` for the wire;
`execution/{types,lower}.rs` for the closed plan; and the existing Rust
`service/{endpoint,process,readiness,registry}.rs` owners for observation,
round execution, and transaction. Delete superseded inventory-dependent
readers, TCP probe branches, and old event writers in that same cutover. Edit
the options source and regenerate `OPTIONS.md`; do not hand-edit generated
output. Old manifest bytes must reject rather than silently acquire new
semantics. The proposal does not itself change normative behavior.

## Implementation proof gates

1. **Production OS observer:** carry the successful macOS FD-only prototype
   into the Rust observer without a PCB dependency. Check the decoded record
   length and SDK layout; reproduce exact IPv4/IPv6, both IPv6-only modes,
   `LISTEN`, wildcard rejection, replacement detection, and process-group
   members. Add controlled process-tree descendants and fault injection for
   FD loss, process exit and PID reuse, denied inspection, malformed or
   unsupported layout, and bounded list growth. A missing or partial host PCB
   stream must be irrelevant. Prove the corresponding positive-only Linux
   path and its inspection-error handling. Repeat on supported macOS host
   policies; this one-host prototype is not release coverage.
2. **Admission and authoring:** Nix and raw Rust reject omitted or misplaced
   endpoint probes, partial multi-endpoint coverage, the removed TCP-only form,
   endpoint-less mixes, invalid invocation references, and malformed wire
   records before child effects. Verify one-endpoint and named-endpoint
   normalization, attached bare placeholder resolution for a nonprimary
   endpoint, positive policy bounds, exact inventory coverage, and generated
   freshness.
3. **Runtime semantics:** an exact managed listener plus its application probe
   is needed for each endpoint. A process that stays live without listening,
   a wildcard-only managed listener, or failure of a nonprimary probe cannot
   commit partial readiness or release dependent tasks. Check socket identity
   replacement during the *last* probe and across the full set, so an earlier
   successful probe is never reused for a new socket. Check liveness and
   containment during probes and immediately before the atomic ready
   transaction. Exercise whole-round retry counts, endpoint-less behavior,
   health, cancellation, capture failures, recovery, and registry fault
   injection.
4. **Known limits:** prove `SO_REUSEADDR`/`TIME_WAIT` restart and bind refusal;
   demonstrate wildcard/exact coexistence and an old or shared-socket responder
   without claiming exclusive ownership or response attribution. Cover both
   IPv6-only modes and exact IPv4/IPv6 address matching. Verify that a partial
   unrelated host inventory does not turn a positive managed witness into a
   false refusal or a negative host claim.
5. **Cross-layer delivery:** attach Postgres's existing protocol check to its
   endpoint and package the three demonstrated Reth checks as declared probe
   invocations; update synthetic fixtures. Test a JSON-RPC HTTP 200 error,
   missing and incorrect JWT, incorrect WebSocket upgrade, wrong response ID,
   and nonprimary probe failure. Reconcile Reth's phase budget with the runtime
   gate timeout. Run focused Nix, manifest, runtime, and raw event/output
   tests, then the cross-layer `.#ci -- --dirty` gate on Linux and macOS.
   Report release and integration coverage separately.

Fileport-only listeners remain outside the initial supported set. Responder
identity remains an explicit limit of opaque application invocations; an
adapter that needs it must provide an instance-specific challenge and proof.
