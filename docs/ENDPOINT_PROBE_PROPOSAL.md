# Application-probed endpoint readiness proposal

Status: reviewed discussion draft, 29 September 2026. The product guarantee
remains undecided. This proposal does not change the shipped contract or permit
endpoint starts that currently return `PORT_UNVERIFIABLE`.

## Motivation

On the investigated macOS 27 host, the unprivileged TCP PCB inventory omitted
a known live child listener. A privileged observer can see known listeners, but
the investigation has not established a complete negative inventory. The
current PORT-1 ownership proof therefore fails closed on this host. See
[`PROBLEM_MACOS.md`](../PROBLEM_MACOS.md) and the
[`privileged observer design review`](PORT_OBSERVER_DESIGN_REVIEW.md).

The proposed alternative is to make an application-defined protocol probe a
required part of every endpoint-backed service's readiness workflow. A failed
exact bind may immediately refuse startup; a successful bind is only a
preliminary signal. The runtime starts the declared service, observes its
process and containment, and accepts readiness only after the application
probes pass at the planned addresses. This avoids making a complete privileged
host listener inventory a prerequisite for ordinary endpoint use.

## Proposed guarantee and owner

For each declared endpoint, readiness means that the runtime still owns a live,
contained service process and the application-defined probe assigned to that
endpoint has exited successfully in this startup attempt. A TCP connect alone
is not an application check. The runtime owns process liveness, containment,
probe execution, ordering, timeout, cancellation, and the readiness commit.
The authoring and manifest boundaries require a probe declaration for every
endpoint before any service preparation or child spawn. An endpoint-less
service retains invocation-based readiness without an addressability claim.
An arbitrary invocation can ignore its assigned address or exit successfully
without checking a response. Nixfied can validate declaration coverage and
record probe outcomes; the author or adapter owns the command's meaning.

This is **not** the current PORT-1 guarantee. A protocol response can come from
an earlier or unrelated copy of the same application while the newly started
process remains live but does not own the socket. A successful application
probe proves useful service behavior at an address; it does not identify the
kernel socket or its owning process. The contract, output, and user guidance
must state that distinction if this alternative is adopted. The runtime must
not emit `port.owner-verified` or equivalent ownership evidence on the strength
of a probe alone.

## Candidate workflow

1. During Nix validation and independent Rust admission, reject an
   endpoint-backed service without a declared application probe assigned to
   each endpoint. Reject a TCP-connect-only readiness probe. The review's
   smallest checkable candidate is a map keyed exactly by declared endpoint
   IDs, with one exec probe policy per key. This proves declaration coverage
   and gives each endpoint an independent outcome, but cannot prove what the
   opaque command actually did. Endpoint-less invocation readiness is the
   other variant.
2. Keep host endpoint locking for starts that Nixfied can coordinate across
   state roots. Attempt an exact bind with `SO_REUSEADDR` so compatible
   `TIME_WAIT` state does not block restart. An unavailable bind refuses before
   preparation. A successful bind does not certify absence of a wildcard
   listener. Decide whether the bind result is diagnostic only or an admission
   gate; either choice needs cross-platform proof.
3. Start and register the service using the existing process, containment,
   capture, and cleanup lifecycle. During every probe attempt and immediately
   before ready commit, check that the service remains live and contained.
   Failed, timed-out, cancelled, or unprovable checks cannot commit readiness.
4. Run the declared protocol checks against the planned endpoint addresses.
   Dependent tasks start only after all required checks pass. Continue existing
   health, teardown, recovery, and state obligations; specify how subsequent
   health checks cover multiple endpoints.

No check based only on a port connection, process name, PID, or successful
bind should be described as service ownership.

## Boundary cases to resolve

- **Wrong responder:** An old instance may answer the same health request while
  the new child remains live without listening. Ordinary protocol probes do
  not reject this. The review must decide whether this weaker result is an
  acceptable product guarantee. If exact instance identity remains required,
  the design needs an independent mechanism, such as a fresh per-run challenge
  supported by the application or a runtime-bound socket handed to the child.
- **Wildcard coexistence:** An exact bind can succeed beside a wildcard
  listener with `SO_REUSEADDR`. The proposal would need to accept and document
  that coexistence, or retain a reliable conflict observation. The exact
  listener normally receives traffic for its address, but a probe still does
  not attribute that listener to Nixfied's child.
- **Effects before failure:** Removing complete preflight observation means
  prepare, state changes, and spawn may occur before a probe detects failure.
  Existing failure cleanup must remain reliable. The former guarantee of
  rejecting all stable listener conflicts before prepare would be removed.
- **Application integration:** Probe declarations must be concrete and
  repeatable for generic services, including multi-endpoint services. A check
  that ignores its assigned address, or merely returns success, gives no useful
  endpoint evidence. Today ready and health each have one probe, and the Reth
  adapter declares HTTP, WebSocket, and auth RPC endpoints while probing only
  HTTP. The proposal needs a meaningful probe for each endpoint, and a decision
  about whether subsequent health checks repeat all of them.
- **Error vocabulary:** `PORT_CONFLICT`, `PORT_UNVERIFIABLE`, readiness errors,
  registry events, and summaries currently encode ownership semantics. A
  contract change needs an atomic replacement of meanings and coupled tests,
  not an interpretation of old records as new evidence.
- **Race and interference:** Other host processes are outside the Nixfied
  endpoint lock. A preflight bind closes before child startup; an outside
  listener can appear afterward. Probe behavior is the proposed acceptance
  criterion, with the wrong-responder limitation above.

## Architecture review questions

1. Is application responsiveness plus owned-process liveness and containment
   a sufficient guarantee for Nixfied's intended `dev`, `test`, and `ci` uses?
   Which user-visible claims depend on exact socket attribution?
2. Can probe coverage be made explicit for every declared endpoint using the
   existing lifecycle and manifest concepts, or would the new declaration
   shape increase complexity more than the macOS observer path?
3. Which preflight bind behavior and error precedence remain useful once a
   complete host listener inventory is no longer required?
4. If exact identity is required, is a fresh challenge practical for the
   adapters Nixfied supports, or is socket handoff the smaller coherent design?
5. What independent wrong-responder and cross-platform tests would disprove
   the chosen guarantee?

## Architecture review findings

Two independent reviews agreed that required protocol probes are coherent only
with an explicit change from exact socket ownership to application-probe
success plus owned-process liveness and containment. An old wildcard listener
can answer after a successful exact preflight bind if the new child fails to
listen but remains live. Another process can also bind after preflight closes
its socket. A mandatory ordinary probe does not exclude either case.

The reviews recommend one probe outcome per declared endpoint rather than an
opaque single command that claims to cover several endpoints. They also found
that the current registry commits `port.owner-verified` with exact endpoint
evidence. A probe-only cutover must replace that event and its projections,
never reuse ownership wording for historical or new probe outcomes. The
contract, ABI inventory, Nix declarations, Rust admission and lifecycle,
adapters, help, documentation, and tests form one atomic change.

The minimum proof set includes Nix and raw Rust rejection of missing, TCP-only,
and incomplete probe declarations; each Reth endpoint succeeding and one
nonprimary failure blocking dependent tasks; wrong-responder behavior that
demonstrates the accepted limit; wildcard coexistence; bind refusal before
prepare; cancellation and containment failure during probes; and absence of
ownership events or claims from probe-only readiness on Linux and macOS.

Do not implement this proposal until the guarantee and its ownership boundary
are accepted, `docs/CONTRACT.md` is revised, and all manifest/runtime ABI,
adapters, documentation, and proof surfaces are planned as one cutover.
