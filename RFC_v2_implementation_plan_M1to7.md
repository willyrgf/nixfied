# RFC v2 Implementation Plan: Pre-M1 Through M7

Source of truth for scope and architecture remains `RFC_v2.md`.

## Current Progress Checkpoint

- Completed in this repository: M0, M1, M2, and M5 implementation and proof scripting.
- Blocked pending runtime/model generalization: M3 (postgres adapter), M4 (workflow graphs), M6 (polyglot example).
- Deferred by decision: M7 (optional manifest envelope).
- `tests/m3/prove-postgres-adapter.sh`, `tests/m4/prove-workflow-graphs.sh`, and
  `tests/m6/prove-polyglot-stack.sh` are explicit non-blocking placeholders.
  `tests/m7/prove-optional-manifest-envelope.sh` remains optional and intentionally
  skipped.

## Agenda

This continuation plan covers milestone work through M7 and keeps M0 constraints.

### Scope

*1. M1: slots, per-slot state pathing, and registry partitioning.*
*2. M2: cancellation, lease reconciliation, cleanup hardening, and generic lifecycle*
*3. M3: Nix-side reference adapter, minimal postgres*
*4. M4: workflow graphs*
*5. M5: installable downstream wrapper hardening*
*6. M6: polyglot example*
*7. M7: optional manifest envelope (only if triggered by concrete need)*

The optional runtime adapter protocol remains deferred beyond this plan's main M1-M7
line. It is included as an appendix because RFC v2 lists it as a possible future
milestone if direct generic runtime execution becomes insufficient.

## M1: Explicit Slot Constraints And Per-Slot State

### Product Purpose

Enable two slots of the same environment in the same checkout with deterministic
placement and registry isolation, while preserving M0 runtime boundaries.

### Strict Boundary

Included:

- Slot policy surface updates for admission and state path derivation.
- Per-slot state roots under `state/<project>/<environment>/<slot>`.
- Per-slot registry file ownership and process records.
- Deterministic port windows and slot-local readiness/port reservations.

Excluded:

- Multi-project or multi-environment orchestration beyond what M1 explicitly scopes.
- Cross-slot service reuse.
- Runtime protocol changes outside registry/process/cleanup scope.

### `Runtime` / `nixfied-model` Changes

Minimal schema and admission changes already made in prior steps for slot-aware model
interpretation and per-slot path materialization.

### `Nix` Module / Compiler Changes

- `slotPolicy` values (`min`, `default`, `max`) drive slot materialization.
- `slotPlacements` are emitted when slot cardinality > 1.
- Validation remains exact for M0/M1 constants where applicable.

### Model Invariants

- `slotPolicy.min <= slotPolicy.default <= slotPolicy.max`.
- Slot indices are contiguous from 0..max.
- Per-slot placement must remain deterministic and host-absolute free.

### `nixfied-runtime` Changes

- Admission validates the requested slot against policy and constraints.
- Registry/state paths are materialized as slot-specific roots.
- `ps` and `down` are slot-aware.

### Risks And Mitigations

Risk: slot drift in placement leads to collision or registry aliasing.

Mitigation: deterministic slot windows and per-slot state root path derivation.

### Proof Script

- `tests/m1/prove-slot-isolation.sh`

### Dependencies

- Requires M0 runtime model and registry shape.
