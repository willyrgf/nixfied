# Managed-listener and application-probed endpoint readiness

Status: accepted design direction, 29 September 2026; details and proof gates
remain open. This is a proposal, not the shipped contract. Until a complete
cutover is implemented and verified, [`CONTRACT.md`](CONTRACT.md) remains
normative and endpoint starts that return `PORT_UNVERIFIABLE` still refuse.

## Decision to develop

Replace PORT-1's required **negative host-wide listener inventory** with a
**positive observation of a listener held by the managed service**, plus a
required application probe for each declared endpoint. Keep one revised
guarantee on Linux and macOS. This retains a kernel-backed connection between
the declared endpoint and the service process while avoiding a dependency on
unproven privileged host-wide absence evidence.

Proposed readiness invariant, for every declared endpoint:

1. The service's recorded process is live and contained. A process in that
   service's verified containment holds an inspectable TCP socket in `LISTEN`
   state at the planned **exact** address, family, and port. A wildcard-only
   socket does not satisfy the declaration.
2. The application-defined ready probe attached to that endpoint succeeds in
   this startup attempt. The author or adapter must make it check the assigned
   address and meaningful protocol behavior; the runtime can prove its exit
   status and attachment, not the meaning of arbitrary command bytes.
3. The runtime rechecks process identity, containment, and the listener witness
   before atomically recording the complete endpoint set and ready transition.
   Failure or uncertainty cannot commit readiness or start dependent work.

Health repeats the per-endpoint observation and application probes at the
existing health phase. It does not introduce a continuous health daemon.
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

This evidence supports a candidate design, not an implementation proof. The
checked-in macOS feasibility probe correlates a known child FD to a PCB socket
handle. It does not yet prove that an FD-only decoder can establish the exact
tuple and listen state on a host where the PCB body omits that child. That is
the first implementation gate below.

## Owners and rejection boundaries

| Fact or invariant | Owner | Rejection boundary |
| --- | --- | --- |
| Each endpoint has one ready and one health application probe | Nix declarations/compiler and independent Rust manifest validation | Reject absent, extra, misplaced, or incoherent probes before prepare, state mutation, or spawn |
| Planned address and endpoint set | Existing Nix derivation and Rust admission/selection | Reject invalid references, port plans, and declaration/plan mismatches before effects |
| Startup coordination and exact-bind availability | Rust endpoint owner | Reject lock contention or unavailable bind before service prepare; bind success is not absence proof |
| Live process, containment, and exact listener witness | Rust runtime against OS process/socket facts | Reject unprovable inspection before ready commit; wait for an absent witness only when inspection is complete for the relevant managed processes |
| Probe attempt, deadline, cancellation, capture, and settlement | Existing Rust lifecycle owner | Failed or unprovable attempt cannot advance readiness or health |
| Protocol meaning and target use within an opaque invocation | Service author or adapter | Adapter tests prove concrete commands; runtime validates attachment and observes exit, but cannot infer command semantics |
| Durable endpoint evidence and ready status | Existing transactional registry owner | Commit the exact endpoint set and ready status together, or neither |

The accepted listener witness is **positive**. The runtime need not enumerate
unrelated processes or establish an empty host-wide PCB stream. It must not
turn a failed inspection of a candidate managed process into an empty FD list.
If a service has no accepted witness after a complete relevant inspection,
readiness remains pending until its deadline. If inspection is denied,
malformed, unsupported, or cannot establish the process identity and
containment, readiness is unprovable rather than merely slow.

## Candidate authoring and execution shape

Attach required `readyProbe` and `healthProbe` to each endpoint declaration,
including the single-endpoint shorthand. Normalize both forms to one manifest
`Endpoint` record. Structural attachment avoids a separate endpoint-ID map and
its key-set reconciliation. Keep `lifecycle.ready` and `lifecycle.health` as
the owners of one operation ID and terminal policy per phase; their scalar
probe belongs only to the endpoint-less alternative. Rust lowering should
construct a closed `Endpointless(invocation probe)` or
`Endpoints(nonempty endpoint/probe set)` plan. Reject the old TCP-connect-only
probe form in the same exact-ABI cutover. A bare TCP connection does not test
application readiness.

Keep the fixed host startup lock from preflight through ready commit or failed
startup cleanup. Attempt an exact bind with `SO_REUSEADDR` for each endpoint so
compatible `TIME_WAIT` restarts remain possible. Treat `EADDRINUSE` as an
unavailable bind before preparation, without claiming to have identified its
owner. The preflight socket closes before the child starts; an unrelated host
process can still race. A successful bind never establishes a free host port
or excludes wildcard coexistence. The core startup path should not require
`pcblist_n`, `SOCK_DIAG`, a full process scan, or sudo merely to accept an empty
host result. Such observations may aid diagnostics only if their limits are
explicit and they are not promoted into negative authority.

For readiness, inspect the known service process and eligible processes found
through its existing containment tracking. On macOS, the first candidate is
`proc_pidinfo(PROC_PIDLISTFDS)` plus
`proc_pidfdinfo(PROC_PIDFDSOCKETINFO)` for those PIDs. Decode a complete TCP
socket record, require `LISTEN` and the exact planned local tuple, and retain
socket identity and generation where the OS supplies them. On Linux, use the
existing kernel TCP and process-FD mechanisms for the same **positive**
witness; a host-wide absence claim must not remain a platform-specific
requirement. Bound list growth, reject malformed records, and verify PID/start
identity and containment around observations. Reobserve the same witness
around the application probe and immediately before commit to detect ordinary
socket replacement. These checks narrow races; they do not make responder
attribution or continuous socket ownership atomic.

Run attached probes in canonical endpoint-ID order with each endpoint's own
attempt policy. Reuse the existing invocation preparation, process registration,
capture, cancellation, liveness, containment, and cleanup machinery. Check
those obligations during waits and after attempts. Run the corresponding
health set in the same order. A service with a listener retained only by a
fileport or a queued socket right, and no inspectable managed listening FD,
does not meet this initial candidate; accepting such a form would require a
separate positive witness design and proof.

## Evidence and ABI cutover

Process-bound endpoint rows continue to record planned coordinates and the
service process association. At ready commit, compare the exact endpoint set
against those rows and transactionally record, for each endpoint, the managed
listener witness and successful application-probe outcome with the ready
transition. A candidate new event is `endpoint.readiness-observed`; its payload
must name the endpoint and distinguish socket/process observation from probe
outcome. The final event name and minimal redaction-safe payload need review.
Do not emit new `port.owner-verified` events under the revised meaning, and do
not reinterpret or rewrite historical events. Readers and summaries must
distinguish historical ownership evidence from the new readiness evidence.

`PORT_UNVERIFIABLE` remains appropriate for denied or incoherent required
managed-process/socket inspection. An absent exact witness after a complete
relevant inspection, or failed application probes, ends as a readiness failure
after the configured attempts. Lock contention and unavailable bind remain
preparation-time refusals. The existing `PORT_CONFLICT` reason vocabulary
asserts a listener in cases where the new path may know only that bind failed;
revise its reason and output semantics rather than mislabeling that fact.
Preserve existing cancellation, process escape, capture, and settlement
priorities when placing the new observation in the lifecycle.

This is one contract/ABI cutover, not a macOS-only fallback. Revise
[`CONTRACT.md`](CONTRACT.md), architecture, the authored
[`capability.txt`](../runtime/crates/nixfied-manifest/capability.txt), Nix/Rust
declarations and validators, ABI snapshot, fixtures, output readers, adapter
guidance, and gates together. Delete superseded inventory-dependent readers
only after the new ready transaction and behavioral proof exist. Edit the
options source and regenerate `OPTIONS.md`; do not hand-edit generated output.
Old manifest bytes must reject rather than silently acquire new semantics.

## Proof gates

1. **macOS feasibility:** with no sudo, independently decode `LISTEN`, exact
   IPv4/IPv6 local tuple, bind mode, and stable socket identity from a known
   child's FD. Prove the runtime's positive path is independent of a missing
   or incomplete host PCB stream by injecting that condition. Repeat on a
   host or context that omits the known child if one is reproducible. Exercise
   direct child and contained descendant, FD loss/replacement, process exit,
   PID reuse, denied inspection, malformed/unsupported layout, and list growth.
   Establish which OS calls succeed under supported host policies; a header
   layout alone is not proof.
2. **Admission and authoring:** Nix and raw Rust reject omitted or misplaced
   endpoint probes, partial multi-endpoint coverage, the removed TCP-only form,
   endpoint-less mixes, invalid invocation references, and malformed wire
   records before child effects. Verify one-endpoint and named-endpoint
   normalization, exact inventory coverage, and generated freshness.
3. **Runtime semantics:** an exact managed listener plus its application probe
   is needed for each endpoint. A process that stays live without listening,
   a wildcard-only managed listener, or failure of a nonprimary probe cannot
   commit partial readiness or release dependent tasks. Check socket identity
   replacement during probes and liveness/containment immediately before the
   atomic ready transaction. Exercise health, cancellation, capture failures,
   recovery, and registry fault injection.
4. **Known limits:** prove `SO_REUSEADDR`/`TIME_WAIT` restart and bind refusal;
   demonstrate wildcard/exact coexistence and an old or shared-socket responder
   without claiming exclusive ownership or response attribution. Cover both
   IPv6-only modes and exact IPv4/IPv6 address matching. Verify that a partial
   unrelated host inventory does not turn a positive managed witness into a
   false refusal or a negative host claim.
5. **Cross-layer delivery:** give Postgres its existing protocol check on its
   endpoint; provide meaningful checks for Reth's HTTP, WebSocket, and
   authenticated RPC endpoints; update synthetic fixtures. Run focused Nix,
   manifest, runtime, and raw event/output tests, then the cross-layer
   `.#ci -- --dirty` gate on Linux and macOS. Report release and integration
   coverage separately.

## Questions for the next review

- Does a process-scoped FD witness remain available on a macOS host or context
  where `pcblist_n` omits that very listener? Direct FD proof and injected
  stream independence are required first; reproduce the old access behavior
  where possible and retain fail-closed inspection errors.
- Is the initial FD-only supported set sufficient for Nixfied's declared
  services, or is a positively inspectable fileport-held listener required?
- Which exact `PORT_CONFLICT` reason, event name, and output fields communicate
  bind unavailability and the revised readiness evidence without implying
  exclusive ownership?
- Do any supported adapters need an instance-specific challenge? If so, define
  that in the adapter's protocol rather than claiming an opaque exec probe
  generally proves responder identity.
