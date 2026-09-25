# Known gaps

This document records known limitations and deferred work. It does not change
the normative [contract](CONTRACT.md) or authorize an implementation.

## Incomplete semantic parity validation between framework and runtime

**Status:** open; broader runtime-consumption enforcement is separate from the
delivered authoring reference and shared structural declarations.

### Gap

Nixfied lacks comprehensive validation that the meaning exposed by its Nix
authoring surface agrees with how the Rust runtime validates, lowers, and uses
each relevant field. Successful serialization, admission, or documentation
generation does not by itself prove that a declared effect occurs at runtime.

The intended invariant is that supported inputs have the documented effects,
constraints, defaults, and rejection phases across the framework/runtime
boundary. Intentional differences must be explicit: Nix-only app metadata need
not reach the manifest, and a serialized field need not affect execution or identity.
Parity does not require identical Nix and Rust representations.

### Existing protections and their limits

- The authored [capability inventory](../runtime/crates/nixfied-manifest/capability.txt)
  derives the exact runtime ABI. It records contract changes; it cannot detect
  every semantic change that was not recorded in its bytes.
- The [whole-inventory coverage audit](../nix/checks/coverage.nix) maps every
  inventory record to its wire declaration. This does not establish that runtime behavior honors those
  fields' documented meaning.
- Typed decoding, manifest validation, and
  [execution lowering](../runtime/crates/nixfied-runtime/src/execution/lower.rs)
  reject invalid input. Exhaustive destructuring requires a field-handling
  decision, but that decision can explicitly discard a field.
- Nix and Rust independently derive selected graph facts under
  [DERIVE-1](CONTRACT.md) and the [derivation specification](DERIVATION_SPEC.md).
  Those checks and focused behavioral tests provide real parity evidence for
  their covered cases, without establishing complete coverage of all effects.

### Concrete evidence

Service `stateRefs` are serialized manifest data that execution lowering
discards; [primitives.nix](../nix/modules/primitives.nix) and the
[state guide](GUIDE.md#services-slots-and-state) describe that role. The
`descriptive_refs_change_manifest_bytes_but_not_lowered_service` test in
[execution lowering](../runtime/crates/nixfied-runtime/src/execution/lower.rs)
independently proves that changed manifest bytes leave the lowered service
contract unchanged. Shared structural declarations alone could not establish
that distinction.

### Ownership and follow-up evidence

Nix declarations and lowering own the authoring-to-manifest boundary. The shared
manifest types and Rust admission, lowering, and execution own their respective
runtime boundaries. A future parity effort should identify the relevant owner
and rejection phase for each covered promise, then establish:

- Agreed accepted/rejected inputs, including intentional representation and
  validation differences, defaults, omission, and null behavior.
- Explicit treatment of fields affecting execution, identity, or output, and
  an explanation for intentional omission at a boundary.
- Independent behavioral tests showing promised effects and non-effects. For
  `stateRefs`, distinguish raw manifest hash changes from the lowered service contract.
- Preservation of phase-specific rejection and existing independent graph
  derivation checks.

The meta-framework can reduce structural drift through shared declarations and
generated bindings. That alone cannot prove meaningful runtime consumption.
Reading a field, passing it to a function, or generating both an implementation
and its expected test result from one declaration is insufficient behavioral
evidence. Any stronger enforcement mechanism needs a separately scoped design;
this gap does not prescribe a runtime interpreter or per-field tracking system.

Correct inaccurate explanations independently of that broader work. Changes to
manifest fields, identity, or runtime behavior follow the existing atomic contract
procedure and verification guidance in [DEVELOPMENT.md](DEVELOPMENT.md).

## Unproven runtime guarantees

The session-ownership runtime has these open platform and proof limits:

- **macOS coverage:** the session-ownership commits were built and tested on
  Linux only; CI macOS coverage is still required. The Linux-only `statx`
  mount-root check has a macOS fallback that relies on the device comparison
  alone.
- **Power-loss durability:** deletion and marker publication use `fsync`
  ordering, which supports only process-death recovery. Host power-loss
  durability is not claimed.
- **Unsupported filesystems:** only known network filesystems are refused.
  Other filesystems are assumed to provide local `flock`, `rename`, and `fsync`
  semantics.
- **Supervision shape:** there is no single in-memory supervisor collection
  that owns all children. Checkpoints meet the requirement instead: every wait,
  release, startup grace, and node success observes every started service. The
  owner does not poll presenter status at checkpoints.
- **Concurrent teardown observation:** ordered teardown observes every
  remaining service before each stop signal. A service that exits while
  another service stops is classified as failed from its own exit when that
  sweep or its own stop observes it; no concurrent observation runs inside one
  service's stop wait.
- **Owner-kill proofs:** tests kill the owner during preparation,
  registration, readiness, background owner death, and interrupted deletion
  (pending intents and injected commit failures). No test kills a process during
  a real deletion walk, or at each handshake boundary of the background launch
  with a deterministic barrier.
- **Success checkpoint after task exit:** it has no deterministic black-box
  proof. The window between the task loop's last checkpoint and its exit
  observation cannot be forced from outside the runtime.

## Separately scoped review candidates

These are not merge blockers or approved implementation assignments. Each
candidate needs its own invariant, owner, rejection boundary and proof. Do not
bundle unrelated changes merely to share an ABI rotation. Wire removals follow
the atomic contract procedure in [DEVELOPMENT.md](DEVELOPMENT.md).

### Authoring and compiler boundary

- **Flat authoring alternatives:** [primitives.nix](../nix/modules/primitives.nix)
  combines a `kind` discriminator with nullable or defaulted payload fields for
  tasks, probes, endpoint topology, and secret resolvers. Nix projection
  silently drops some inapplicable values before emission: composite
  `exitPolicy` and `artifactRefs`/`logRefs`/`summaryRefs`, a `tcp` probe's
  `invocation`, `primaryEndpoint` beside the singular `endpoint` form, and
  `primaryEndpoint` on an endpoint-less service. Nix does not check that a
  multi-endpoint `primaryEndpoint` names a declared endpoint; Rust structural
  validation still rejects it. Native tagged alternatives (`types.attrTag`) would make
  these combinations unrepresentable; relational checks stay explicit.
- **Configured-value barrier:** `resolve.nix` returns a lazy configuration and
  `validate.nix` forces only the values its checks read. An invalid value that
  no check or projection reads is never evaluated.
  `surfaceDescriptionsForced` is an isolated workaround for one case. One
  compilation-only forcing boundary before relational validation would close
  this without forcing package internals or metadata-only reference queries.
- **Descriptive refs:** service `stateRefs`/`logRefs` and task
  `artifactRefs`/`logRefs`/`summaryRefs` are manifest/ABI data that execution
  lowering discards.
- **Unused invocation deadlines:** the shared invocation `timeoutMs` is
  accepted and serialized for service start and exec-probe invocations, but
  lowering discards it there. Only leaf tasks consume it; probe attempts use
  `probe.timeoutMs`.
- **Unused terminal labels:** lifecycle `terminal.success`/`failure` tokens are
  configurable event labels and do not define outcomes.
  Option: a fixed runtime outcome vocabulary.
- **Closure metadata:** closure `kind` (`executable`/`helper`) selects nothing
  at runtime. Closure `effects` other than `network-listener` are attestations
  with no consumer. The `network-listener` attestation may also be redundant
  where endpoint declarations already express addressability; endpoint
  ownership checks remain either way.
- **Source policy knobs:** `admissionFingerprintPolicy` accepts any nonempty
  string and computes or compares no fingerprint. `dirtyPolicy = "warn"`
  behaves like `allow`. In `live-workspace` mode, `sourceIdentity` is recorded
  but does not select or verify the root. `snapshot` and `flake-input` share one
  immutable-root resolver and differ only in provenance.
- **Placeholder typos:** define reserved grammar and literal child-program syntax
  before considering rejection of unknown `${...}` forms.
- **Declaration diagnostics:** consider naming offending references and legal
  alternatives more precisely; diagnostics do not replace discovery.

### Adapters and commands

- **Adapter host propagation:** the synthetic and PostgreSQL adapters hardcode
  `127.0.0.1` in child arguments and probes, and the Reth adapter defaults to it
  without passing the declared host. A declared endpoint host override changes
  runtime planning and ownership checks but not these children.
- **Adapter catalog:** consider a derived view of adapter defaults, clearly
  distinguished from supported module overrides and native wrapper conventions.
- **Framework `.#check` arguments:** the root check app ignores all arguments,
  including `--help`.
- **Installer report:** with an existing `nixfied.nix`, `install` preserves the
  module but still prints the requested `projectId` and `name` as if applied.

## PostgreSQL lifecycle test reliability

One integration run timed out after 90 seconds waiting for PostgreSQL startup.
The focused test and full CI then passed on the unchanged tree. The cause remains
unestablished; the retries do not prove an environmental cause or a runtime fix.
If it recurs, retain startup diagnostics when investigating
`interrupt_and_recover_stops_orphan_and_starts_fresh_postgres` in
[lifecycle.rs](../runtime/crates/nixfied-runtime/tests/lifecycle.rs). This isolated
observation is separate from the completed reference feature.
