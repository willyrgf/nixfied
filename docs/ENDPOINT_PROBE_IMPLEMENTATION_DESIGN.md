# Endpoint probe implementation design

Status: architecture recommendation, 29 September 2026. This is a design for
the alternative in [`ENDPOINT_PROBE_PROPOSAL.md`](ENDPOINT_PROBE_PROPOSAL.md),
not a change to the shipped contract. Implementation is gated on accepting the
revised product guarantee below.

## Decision and invariant

If adopted, an endpoint-backed service is ready only after its session-owned
process is live and contained and the application-defined ready probe attached
to **every** declared endpoint has succeeded during this startup. Health repeats
the same per-endpoint coverage after readiness, before dependent work. Probe
attempts, cancellation, containment checks, and final ready commit stay with
Rust. Nix validation and independent Rust admission reject missing, misplaced,
or incoherent probe declarations before preparation, state changes, or spawn.

This guarantees authored probe success at each planned address **only to the
extent the authored command actually checks it**. Nixfied can verify an
invocation's declared position and exit status; it cannot infer which address
an arbitrary command contacted. An old instance may answer a valid protocol
probe while the new child remains live but fails to listen. Therefore this
design changes PORT-1: it does not prove exact kernel socket ownership,
host-wide absence of wildcard conflicts, or responder identity. The contract
and user-facing evidence must say so. If those claims remain required, this
design must not replace PORT-1; use an independently specified instance
challenge or socket handoff instead.

## One owner for each probe policy

Attach required `readyProbe` and `healthProbe` to each declared endpoint. The
single-endpoint shorthand carries them beside `endpointId` and `host`; each
member of the named `endpoints` map carries them beside `host`. The compiler
normalizes both forms to one `Endpoint` wire record containing `endpointId`,
`host`, `readyProbe`, and `healthProbe`. This makes endpoint coverage structural:
no separate map repeats endpoint IDs or needs key-set reconciliation.

Keep `lifecycle.ready` and `lifecycle.health` as the owners of one operation ID
and terminal semantics per phase. Their scalar `probe` exists only for an
endpoint-less service, which has no endpoint checks to attach. The manifest
validator rejects a scalar probe on an endpoint-backed service or its absence
on an endpoint-less service. Rust lowering constructs a closed alternative:
`Endpointless(invocation probe)` or `Endpoints(nonempty endpoint/probe set)`.
Execution cannot see an incoherent mix. A probe is an invocation with retry,
timeout, and attempt policy; remove the TCP-only probe variant, its `kind`
field, and the optional invocation field in the same ABI cutover. The author or
adapter owns whether each invocation performs a meaningful protocol check
against its attached endpoint. No runtime application-specific branch is added.

This changes the authored manifest inventory in
[`capability.txt`](../runtime/crates/nixfied-manifest/capability.txt), the
corresponding Nix/Rust declarations, generated Rust, fixtures, ABI snapshot,
and generated options. Old bytes are rejected; no alias, fallback, or registry
history reinterpretation is introduced. Keep one service-level operation ID
per phase, so no new per-endpoint operation identity or derivation rule is
needed.

## Startup and probe execution

Keep the host startup lock across exact-bind preflight, prepare, spawn, and
readiness. Keep a `SO_REUSEADDR` exact-bind attempt to allow compatible
`TIME_WAIT` restarts. `EADDRINUSE` can refuse before preparation as an
unavailable bind, without claiming to identify a particular listener. Other
socket-check failures fail closed with a distinct checked outcome. A successful
bind is no proof of a free host port and does not trigger a kernel inventory.
The temporary preflight socket closes before the child starts, so outside
processes can still race the child; the revised probe guarantee is the only
readiness claim.

Prepare each probe invocation once with the existing service substitution
scope. Run attached probes in canonical endpoint-ID order, exhausting one
endpoint's retry policy before moving to the next. Reuse the current exec
probe, cancellation, service liveness, containment, capture, and settlement
machinery. Check the owned service during waits, after attempts, and again
immediately before the atomic ready commit. A failed, timed-out, cancelled, or
unprovable probe prevents readiness and dependent task execution. Run the
health set in the same deterministic order. Today health runs once after ready;
this design does not introduce a continuous health daemon.

Keep process-bound endpoint rows for planned address and cleanup/recovery
association. A row records endpoint coordinates and its process association;
the readiness fact comes from the event committed with it. At ready commit,
compare the exact endpoint set against those rows and atomically record a
probe-success event for each endpoint plus the service ready transition. Do
not emit new `port.owner-verified` events or serialize listener/FD ownership
JSON. Historical `port.owner-verified` events retain their original meaning;
registry readers must distinguish old ownership evidence from new probe
outcomes rather than project either event as the other. Do not rewrite old
event history. Update readers, output projections, errors, summaries, and
tests together. Remove kernel listener observation and its readiness precedence
only after the replacement transaction and proof exist. Delete observer code
that has no remaining consumer.

## Change route

1. **Contract and wire:** revise `docs/CONTRACT.md`, `docs/ARCHITECTURE.md`,
   authored `capability.txt`, Nix spec, manifest types/validation, ABI snapshot,
   exact error/event vocabulary, and raw-wire rejection fixtures as one cutover.
   The product guarantee decision precedes this step.
2. **Nix authoring and compiler:** update `nix/modules/primitives.nix` for both
   endpoint forms; update `nix/compiler/derive.nix` normalization and invocation
   discovery; update `nix/compiler/validate.nix` for probe placement, template
   scope, and operation checks. Regenerate `docs/OPTIONS.md` from its source.
3. **Adapters:** give every declared endpoint a meaningful ready and health
   command. Postgres already has an application-level probe but must move it
   onto its endpoint. Reth needs separate checks for HTTP, WebSocket, and
   authenticated RPC; its present HTTP probe does not cover all three.
   Synthetic fixtures that rely on the default TCP probe need authored commands.
4. **Runtime:** update execution lowering to the closed probe-plan alternative;
   use the existing exec probe path for deterministic per-endpoint sets in
   `service/process.rs`; retain startup locks and bind preflight in
   `service/endpoint.rs`; replace ownership evidence in `service/registry.rs`
   and all readers. Preserve the existing failed-start, cancellation, capture,
   containment, and predecessor-recovery paths.
5. **Public surface and gates:** update README, guide, adapter guidance,
   generated help or views where they describe readiness, and cross-layer
   tests. Run the narrow wire/compiler/runtime proofs before `.#ci -- --dirty`.
   Report macOS and Linux endpoint integration separately.

## Independent proof

- Nix and raw Rust admission reject omitted endpoint probes, TCP-only legacy
  bytes, scalar/endpoint probe coexistence, endpoint-less missing probes, and
  malformed template or closure references before child effects.
- One endpoint and all Reth endpoints can become ready through meaningful
  protocol checks. Failure of a nonprimary endpoint blocks dependent tasks and
  leaves no ready transition or probe-success set committed.
- A controlled wrong-responder case demonstrates the accepted limit: an old
  instance answers while the new child stays live without listening. The test
  must not assert socket ownership or emit an ownership event.
- Wildcard/exact coexistence and successful bind do not become ownership
  evidence. An unavailable exact bind refuses before prepare; compatible
  `TIME_WAIT` permits restart. Test both IPv4 and IPv6 forms where supported.
- Cancellation, child exit, containment escape, timeout, and failed capture
  during an endpoint probe preserve their existing lifecycle priorities and
  cleanup obligations. Registry fault injection proves the endpoint set and
  ready status commit atomically.
- Linux and macOS integration assert the same revised guarantee and raw
  event/output vocabulary. No proof relies on the incomplete macOS listener
  inventory.

## Architecture review outcome

Two independent reviewers converged on endpoint-attached probes, one
service-level operation per phase, and reuse of the existing exec probe and
service lifecycle. Attachment adds fields to the endpoint declaration and
wire record, but removes two possible endpoint-ID maps and their drift checks.
The runtime reviewer found no objection to that shape. Both reviewers require
explicit acceptance of the wrong-responder limit before implementation.
