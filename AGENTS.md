# Agent guide

This file contains the repository rules that should stay in an agent's active
context while editing Nixfied. It is deliberately not the architecture or test
manual. Load the relevant reference below before changing that part of the
project.

## References

| Read | When |
| --- | --- |
| [`README.md`](README.md) | User-facing behavior, installation, and examples |
| [`docs/CONTRACT.md`](docs/CONTRACT.md) | Any model, ABI, lifecycle, output, or public-surface change |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | Rationale behind the contract and its non-goals |
| [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) | Repository map, test placement, gate composition, and build profiles |
| [`docs/DERIVATION_SPEC.md`](docs/DERIVATION_SPEC.md) | Derived graph facts or canonical ordering |
| [`docs/ADAPTERS.md`](docs/ADAPTERS.md) | Adapter authoring or endpoint conventions |
| [`runtime/crates/nixfied-model/capability.txt`](runtime/crates/nixfied-model/capability.txt) | ABI inventory: model/output vocabulary, surfaces, and error codes |

`docs/CONTRACT.md` is the normative behavioral contract. The capability
descriptor is the authored wire-contract inventory whose digest derives
`runtimeAbi`; it is not generated documentation.

## Core guardrails

- Nix evaluates, validates, builds, and realises. Rust admits, executes,
  reconciles, and cleans against `model.json` and OS reality.
- `model.json` is the only semantic seam. Generated schema, docs, and
  capabilities are disposable projections, never additional authority.
- `nixfied-runtime` never invokes Nix or imports Nix expressions. SEAM-1 applies
  to the runtime binary, not to declared child programs.
- The runtime stays domain-generic. Concrete adapters and the public integration
  API stay in typed Nix modules.
- The model algebra remains tasks and services, with inline invocations, static
  composite DAGs, and graph-derived facts. Do not add runtime-owned cache,
  environment-membership, or dynamic orchestration concepts.
- Keep host-absolute placement and secret values out of model data. Child
  environments remain hermetic.
- Do not add v1 compatibility, migrations, sidecars, or shims. Contract changes
  are deliberate ABI changes with coordinated implementation, docs, and tests.
- Admission, endpoint ownership, state cleanup, liveness, containment, and
  secret handling fail closed. Do not weaken their safety gates for convenience.

The short list above is a routing aid, not a substitute for the full contract.

## Change routing

| Change | Primary owner |
| --- | --- |
| User declaration surface | `nix/modules/` |
| Resolution, validation, derivation, emitted model/views | `nix/compiler/` |
| Nix-side model/ABI constants | `nix/spec/` |
| Shared serialized model types and structural validation | `runtime/crates/nixfied-model/` |
| Admission, registry, lifecycle, execution, and controls | `runtime/crates/nixfied-runtime/` |
| Ergonomic model/view/install CLI | `runtime/crates/nixfied-cli/` |
| Domain adapters | `nix/adapters/` |

Keep runtime behavior out of `nixfied-model`; it is the shared typed contract.
Do not invent ad hoc runtime-side model shapes.

## Change rules

- Inspect `git status --short --untracked-files=all` before editing.
- Model types deny unknown fields. Prefer strong types and typed errors; avoid
  panics in library code.
- Treat JSON/text projections and runtime error payloads as stable within the
  exact ABI/toolchain contract. Any model/runtime contract change must be
  recorded in the capability descriptor so the ABI rotates, then update the ABI
  snapshot, both Nix and Rust sides where applicable, contract docs, and tests.
  Bump numeric version components only when their semantics require it.
- When adding a model field, extend the fully populated fixture in
  `runtime/crates/nixfied-runtime/tests/capability_coverage.rs`.
- For derived-fact changes, update `docs/DERIVATION_SPEC.md`, both derivations
  (`nix/compiler/derive.nix` and runtime lowering), and both golden-vector suites.
- For view changes, keep `nix/compiler/views.nix` and the `nixfied` CLI projection
  aligned; never hand-edit emitted models or views. The gate proves emitted and
  runtime-derived views agree.
- Preserve stable error codes and distinguish admission failures from execution
  failures. Never report `MODEL_ADMISSION` after admission.
- Shell must not own graph, registry, validation, liveness, summary, or cleanup
  semantics. Nix must not own live supervision, cancellation, or reconciliation.
- Prefer a complete underlying fix over a workaround or compatibility layer.
  Report anything not fully verified or any remaining fragility.

## Verification

Use the narrowest relevant check while iterating, then widen in proportion to
the change:

```sh
nix run .#check   # hermetic source/build checks + model admission
nix run .#test    # full white-box Cargo floor with required Nix fixtures
nix run .#gate    # runtime-shaped and Nix-layer integration gates
nix run .#ci      # canonical local full-repository gate
```

See `docs/DEVELOPMENT.md` for exact coverage, targeted Cargo commands, the
`--dirty` adoption mode, platform coverage, and the release build. Use `.#test`
rather than treating raw `cargo test` as equivalent: the wrapper supplies the
model required by the interrupt-and-recover lifecycle test.

## Git and handoff

- Do not revert, delete, or overwrite unrelated work.
- Commit only when requested. Keep each commit focused; messages are lower-case,
  one concise line, with no body or trailers.
- Before committing a contract change, review the diff against
  `docs/CONTRACT.md`, scope, tests, and regressions.
- In the handoff, state what changed, which checks ran, and what remains
  unverified.
