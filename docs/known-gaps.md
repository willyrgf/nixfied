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
- [Capability coverage](../runtime/crates/nixfied-runtime/tests/capability_coverage.rs)
  checks fixture field names against inventory tokens. This does not establish
  that runtime behavior honors those fields' documented meaning.
- Typed decoding, manifest validation, and
  [execution lowering](../runtime/crates/nixfied-runtime/src/execution/lower.rs)
  reject invalid input. Exhaustive destructuring requires a field-handling
  decision, but that decision can explicitly discard a field.
- Nix and Rust independently derive selected graph facts under
  [DERIVE-1](CONTRACT.md) and the [derivation specification](DERIVATION_SPEC.md).
  Those checks and focused behavioral tests provide real parity evidence for
  their covered cases, without establishing complete coverage of all effects.

### Concrete evidence

The former `stateRefs` description incorrectly claimed participation in service
identity. [primitives.nix](../nix/modules/primitives.nix) and the
[state guide](GUIDE.md#services-slots-and-state) now explain its actual role:
execution lowering discards it, while it remains serialized manifest data.
The `descriptive_refs_change_manifest_bytes_but_not_service_reuse_identity` test in
[execution lowering](../runtime/crates/nixfied-runtime/src/execution/lower.rs)
independently proves changed manifest bytes with unchanged service reuse identity.

That specific documentation defect is resolved. It illustrates why shared
structural declarations alone cannot establish complete behavioral parity.
Correcting an explanation, changing behavior and removing a field remain
distinct decisions.

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
  `stateRefs`, distinguish raw manifest hash changes from service reuse identity.
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

## Separately scoped review candidates

These were explicitly outside the completed reference delivery. They are not
merge blockers or approved implementation assignments:

- **Descriptive refs:** consider removing service `stateRefs`/`logRefs` and task
  `artifactRefs`/`logRefs`/`summaryRefs`. They remain manifest/ABI data despite their
  execution non-effects; removal requires an atomic contract change.
- **State policies:** review possible consolidation of `cleanupPolicy` and
  `persistence`. Similar cleanup permissions do not establish equivalent service
  identity, marker matching, adoption or existing-state behavior.
- **Placeholder typos:** define reserved grammar and literal child-program syntax
  before considering rejection of unknown `${...}` forms.
- **Declaration diagnostics:** consider naming offending references and legal
  alternatives more precisely; diagnostics do not replace discovery.
- **Adapter catalog:** consider a derived view of adapter defaults, clearly
  distinguished from supported module overrides and native wrapper conventions.

Each candidate needs its own invariant, owner, rejection boundary and proof.
Do not bundle unrelated changes merely to share an ABI rotation.

## PostgreSQL lifecycle test reliability

One integration run timed out after 90 seconds waiting for PostgreSQL startup.
The focused test and full CI then passed on the unchanged tree. The cause remains
unestablished; the retries do not prove an environmental cause or a runtime fix.
If it recurs, retain startup diagnostics when investigating
`interrupt_and_recover_adopts_orphaned_postgres` in
[lifecycle.rs](../runtime/crates/nixfied-runtime/tests/lifecycle.rs). This isolated
observation is separate from the completed reference feature.
