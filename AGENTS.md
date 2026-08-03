# Agent guide

This is Nixfied’s active-context routing sheet. Read the relevant reference
before changing that area. The design and engineering principles below apply
to all code, test, docs, build, and workflow changes.

## Read before changing

| Area | Reference |
| --- | --- |
| User behavior | [`README.md`](README.md) |
| Model, ABI, lifecycle, output, public surface | [`docs/CONTRACT.md`](docs/CONTRACT.md) |
| Architecture and non-goals | [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) |
| Repository map, tests, gates, profiles | [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) |
| Derived facts and ordering | [`docs/DERIVATION_SPEC.md`](docs/DERIVATION_SPEC.md) |
| Adapters and endpoints | [`docs/ADAPTERS.md`](docs/ADAPTERS.md) |
| Generated options | [`docs/OPTIONS.md`](docs/OPTIONS.md) |
| Authored ABI inventory | [`runtime/crates/nixfied-model/capability.txt`](runtime/crates/nixfied-model/capability.txt) |

`docs/CONTRACT.md` is normative. `capability.txt` is the authored wire
inventory whose digest derives `runtimeAbi`, not generated documentation.

## Design and engineering principles

- Keep one current design. Breaking contracts are allowed, but cut over
  completely: update all producers, consumers, tests, fixtures, and docs, then
  delete superseded work. No compatibility shims, aliases, fallbacks, legacy
  readers/decoders, migrations, dual paths, or rollback support. Persisted
  contracts update/reset their baseline and reject old data; never reinterpret
  bytes or rewrite append-only history.
- Optimize for correctness and simplicity: one owner per responsibility,
  guarantees at the strongest practical boundary, and local complete changes.
  Delete duplication and obsolete behavior, not safety, validation, controls,
  tests, or necessary docs. Fix designs fully; report blockers instead of
  adding hacks or partial workarounds.
- Make illegal states unrepresentable. Prefer Rust structs for products,
  enums for alternatives, newtypes/private fields with fallible constructors,
  and ownership, typestate, or structural proofs. Avoid correlated flags and
  `Option` state. Parse untrusted I/O into domain types at boundaries; preserve
  invariants through construction/conversion/default/deserialization; use
  typed redaction-safe errors and explicit checked outcomes. Test rejection
  and invariant-preserving transforms; use compile-fail tests for intentional
  compile-time exclusions. Keep type machinery no more complex than the
  invalid states or change sites it removes.
- Non-trivial work uses dependency-ordered, focused commits with one coherent
  design, its tests/docs/deletions, and atomic inseparable cutovers. If the
  routed references leave architecture unclear, request a read-only review of
  the smallest contract-consistent design and its impact.

## Architecture guardrails

- Nix evaluates, validates, builds, and realises. Rust admits, executes,
  reconciles, and cleans against `model.json` and OS reality.
- `model.json` is the only semantic seam; `views/docs.md` is disposable.
  `nixfied-runtime` never invokes Nix or imports Nix expressions (SEAM-1 is for
  the runtime binary, not declared child programs).
- Keep runtime behavior domain-generic and adapters/API in typed Nix modules.
  The model remains tasks, services, inline invocations, static composite DAGs,
  and derived graph facts; do not add runtime-owned caches, memberships, or
  dynamic orchestration.
- Keep host-absolute placement and secrets out of model data; child
  environments stay hermetic. Admission, endpoint ownership, state cleanup,
  liveness, containment, and secret handling fail closed; never weaken gates.
- Shell must not own graph, registry, validation, liveness, summary, or cleanup;
  Nix must not own supervision, cancellation, or reconciliation. Keep
  `nixfied-model` to the shared typed contract, not runtime behavior or ad hoc
  model shapes.

## Change routing

| Change | Owner |
| --- | --- |
| User declarations | `nix/modules/` |
| Generated apps/help | `nix/project-apps.nix`, `nix/help-*.nix` |
| Resolution, validation, derivation, model/docs view | `nix/compiler/` |
| Nix model/ABI constants | `nix/spec/` |
| Serialized model/structural validation | `runtime/crates/nixfied-model/` |
| Admission, lifecycle, execution, controls | `runtime/crates/nixfied-runtime/` |
| Install CLI | `runtime/crates/nixfied-cli/` |
| Domain adapters | `nix/adapters/` |

## Contract and delivery rules

- Inspect `git status --short --untracked-files=all` first; preserve unrelated
  work. Deny unknown model fields, prefer strong types and typed errors, and
  avoid library panics.
- Contract/ABI changes follow ABI-1: update `capability.txt`, ABI snapshot, Nix
  and Rust constants, architecture/contract docs, and tests. Keep JSON/text/error
  projections consistent; preserve admission versus execution and never report
  `MODEL_ADMISSION` after admission. Bump numeric versions only when semantics
  require it.
- New model fields update the fully populated fixture in
  `runtime/crates/nixfied-runtime/tests/capability_coverage.rs`. Derived facts
  update `docs/DERIVATION_SPEC.md`, both derivations, and both golden suites.
  `nix/compiler/views.nix` is the sole generated-doc renderer; never hand-edit
  emitted models or views.
- Run the narrowest relevant check first, then widen; `.#test` supplies the
  required Nix fixture and is not raw `cargo test`:

  ```sh
  nix run .#check
  nix run .#test
  nix run .#gate
  nix run .#ci
  ```

- Inspect the staged diff before committing, especially contract scope,
  tests, and regressions. Use one concise lower-case commit line with no body
  or trailers. Report changes/deletions, checks run, and remaining unverified
  risks or blockers.
