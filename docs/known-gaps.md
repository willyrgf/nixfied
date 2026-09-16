# Known gaps

This document records known limitations and deferred work. It does not change
the normative [contract](CONTRACT.md) or authorize an implementation.

## Incomplete semantic parity validation between framework and runtime

**Status:** open; broader runtime-consumption enforcement is deferred beyond
the [contract meta-framework RFC](../RFC_EXPOSE_ADOPTER_FACING_API.md).

### Gap

Nixfied lacks comprehensive validation that the meaning exposed by its Nix
authoring surface agrees with how the Rust runtime validates, lowers, and uses
each relevant field. Successful serialization, admission, or documentation
generation does not by itself prove that a declared effect occurs at runtime.

The intended invariant is that supported inputs have the documented effects,
constraints, defaults, and rejection phases across the framework/runtime
boundary. Intentional differences must be explicit: Nix-only app metadata need
not reach the model, and a serialized field need not affect execution or identity.
Parity does not require identical Nix and Rust representations.

### Existing protections and their limits

- The authored [capability inventory](../runtime/crates/nixfied-model/capability.txt)
  derives the exact runtime ABI. It records contract changes; it cannot detect
  every semantic change that was not recorded in its bytes.
- [Capability coverage](../runtime/crates/nixfied-runtime/tests/capability_coverage.rs)
  checks fixture field names against inventory tokens. This does not establish
  that runtime behavior honors those fields' documented meaning.
- Typed decoding, model validation, and
  [execution lowering](../runtime/crates/nixfied-runtime/src/execution/lower.rs)
  reject invalid input. Exhaustive destructuring requires a field-handling
  decision, but that decision can explicitly discard a field.
- Nix and Rust independently derive selected graph facts under
  [DERIVE-1](CONTRACT.md) and the [derivation specification](DERIVATION_SPEC.md).
  Those checks and focused behavioral tests provide real parity evidence for
  their covered cases, without establishing complete coverage of all effects.

### Concrete evidence

The `stateRefs` description in
[primitives.nix](../nix/modules/primitives.nix) claims participation in service
identity. The runtime's
[identity calculation](../runtime/crates/nixfied-runtime/src/service/identity.rs)
excludes it, and execution lowering discards it. The field remains serialized
model data, so changing it changes the raw model hash without changing service
reuse identity through that field.

This demonstrates disagreement between the explanation and implementation.
It does not establish that the runtime should start using `stateRefs`: correcting
the description, changing behavior, and removing the field are distinct decisions.

### Ownership and follow-up evidence

Nix declarations and lowering own the authoring-to-model boundary. The shared
model types and Rust admission, lowering, and execution own their respective
runtime boundaries. A future parity effort should identify the relevant owner
and rejection phase for each covered promise, then establish:

- Agreed accepted/rejected inputs, including intentional representation and
  validation differences, defaults, omission, and null behavior.
- Explicit treatment of fields affecting execution, identity, or output, and
  an explanation for intentional omission at a boundary.
- Independent behavioral tests showing promised effects and non-effects. For
  `stateRefs`, distinguish raw model hash changes from service reuse identity.
- Preservation of phase-specific rejection and existing independent graph
  derivation checks.

The meta-framework can reduce structural drift through shared declarations and
generated bindings. That alone cannot prove meaningful runtime consumption.
Reading a field, passing it to a function, or generating both an implementation
and its expected test result from one declaration is insufficient behavioral
evidence. Any stronger enforcement mechanism needs a separately scoped design;
this gap does not prescribe a runtime interpreter or per-field tracking system.

Correct inaccurate explanations independently of that broader work. Changes to
model fields, identity, or runtime behavior follow the existing atomic contract
procedure and verification guidance in [DEVELOPMENT.md](DEVELOPMENT.md).
