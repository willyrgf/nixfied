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

## Simplification and change policy

- Prefer composition from existing primitives. Optimize for fewer concepts,
  code paths, public types, duplicated responsibilities, and places future
  changes must touch. Prefer deletion and net LOC reduction when behavior and
  proof remain explicit; never trade away type safety, ownership boundaries,
  fail-closed checks, or independent seam verification for fewer lines.
- Backward compatibility is not a design constraint. Deliberate breaking
  changes replace the previous exact contract; old and new contracts are never
  supported concurrently. Follow ABI-1 for the required atomic ABI rotation.
- Delete superseded in-scope code, tests, fixtures, flags, branches, and
  documentation in the same change. Do not retain compatibility fallbacks,
  aliases, shims, commented code, or hidden legacy paths. Git history is the
  recovery mechanism.
- Deliver non-trivial work as dependency-ordered, focused commits. Each commit
  owns one coherent architectural slice, includes its tests, documentation, and
  deletions, passes its focused checks, and leaves the repository coherent. A
  contract transition remains atomic in one commit; do not create WIP commits.
- If architecture or design remains unclear after reading the routed references,
  spawn a read-only architect agent before implementation. Pass this section
  verbatim and ask for the smallest contract-consistent design and its contract
  impact. The primary agent owns the final decision and integration.

## Core guardrails

- Nix evaluates, validates, builds, and realises. Rust admits, executes,
  reconciles, and cleans against `model.json` and OS reality.
- `model.json` is the only semantic seam. Its generated `views/docs.md` is a
  disposable human projection, never additional authority.
- `nixfied-runtime` never invokes Nix or imports Nix expressions. SEAM-1 applies
  to the runtime binary, not to declared child programs.
- The runtime stays domain-generic. Concrete adapters and the public integration
  API stay in typed Nix modules.
- The model algebra remains tasks and services, with inline invocations, static
  composite DAGs, and graph-derived facts. Do not add runtime-owned cache,
  environment-membership, or dynamic orchestration concepts.
- Keep host-absolute placement and secret values out of model data. Child
  environments remain hermetic.
- Admission, endpoint ownership, state cleanup, liveness, containment, and
  secret handling fail closed. Do not weaken their safety gates for convenience.

The short list above is a routing aid, not a substitute for the full contract.

## Change routing

| Change | Primary owner |
| --- | --- |
| User declaration surface | `nix/modules/` |
| Resolution, validation, derivation, emitted model/docs view | `nix/compiler/` |
| Nix-side model/ABI constants | `nix/spec/` |
| Shared serialized model types and structural validation | `runtime/crates/nixfied-model/` |
| Admission, registry, lifecycle, execution, and controls | `runtime/crates/nixfied-runtime/` |
| Install CLI | `runtime/crates/nixfied-cli/` |
| Domain adapters | `nix/adapters/` |

Keep runtime behavior out of `nixfied-model`; it is the shared typed contract.
Do not invent ad hoc runtime-side model shapes.

## Change rules

- Inspect `git status --short --untracked-files=all` before editing.
- Model types deny unknown fields. Prefer strong types and typed errors; avoid
  panics in library code.
- Keep JSON/text projections and runtime error payloads internally consistent
  within the current exact ABI/toolchain contract. They may break through a
  recorded contract change: rotate the ABI, then update the ABI snapshot, both
  Nix and Rust sides where applicable, contract docs, and tests. Bump numeric
  version components only when their semantics require it.
- When adding a model field, extend the fully populated fixture in
  `runtime/crates/nixfied-runtime/tests/capability_coverage.rs`.
- For derived-fact changes, update `docs/DERIVATION_SPEC.md`, both derivations
  (`nix/compiler/derive.nix` and runtime lowering), and both golden-vector suites.
- `nix/compiler/views.nix` is the sole generated documentation renderer. Never
  hand-edit emitted models or views; the gate checks the emitted model and docs
  together.
- Error codes may be renamed or removed through a recorded ABI change. Preserve
  the admission/execution phase distinction; never report `MODEL_ADMISSION`
  after admission.
- Shell must not own graph, registry, validation, liveness, summary, or cleanup
  semantics. Nix must not own live supervision, cancellation, or reconciliation.
- Report anything not fully verified or any remaining fragility.

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

- Delete superseded work within the requested scope; do not revert, overwrite,
  delete, stage, or commit unrelated or pre-existing work.
- Inspect the staged diff before every commit. Messages are lower-case, one
  concise line, with no body or trailers.
- Before committing a contract change, review the diff against
  `docs/CONTRACT.md`, scope, tests, and regressions.
- In the handoff, state what changed, which checks ran, and what remains
  unverified.
