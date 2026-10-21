# Agent guide

This is Nixfied’s active-context routing sheet. Before changing code, tests,
docs, builds, or workflow:

1. Inspect `git status --short --untracked-files=all` and preserve unrelated work.
2. Read the authority routed below.
3. State the invariant, owner, rejection boundary, and proof before editing.

[`docs/CONTRACT.md`](docs/CONTRACT.md) is normative for product behavior and
the manifest/runtime ABI. [`capability.txt`](runtime/crates/nixfied-manifest/capability.txt)
is the authored wire inventory whose digest derives `runtimeAbi`, not generated
documentation.

## Core design

**Design for correctness, low complexity, simplicity, and reuse.** Minimize
concepts, mutable state, indirection, dependencies, coordinated change sites,
and total maintained code together. Prefer the smallest readable implementation
that preserves the contract and its independent proofs. Make invalid states
unrepresentable or reject them at the owning boundary before effects. Give each
fact one owner. Reuse cohesive components through clear interfaces and ordinary
composition; introduce abstractions for demonstrated shared meaning, not
speculative flexibility.

- **Represent the domain.** Use product types for facts that coexist and sum
  types for alternatives. Use newtypes, private constructors, fallible
  conversions, and typestate where they enforce a required invariant more simply
  than the existing design. Match closed alternatives exhaustively. Use `Option`
  only for genuine absence, never for hidden alternatives or correlated state.
- **Close the boundary.** Parse untrusted input into domain types at the
  earliest boundary with enough information. Use fallible validation, lowering,
  or admission for cross-field, graph, host, time, and concurrency invariants
  that types cannot express. Preserve invariants through construction,
  conversion, defaults, and deserialization; reject incoherent input before
  side effects.
- **Give every fact one owner.** Derive repeated facts from one specification.
  Do not add duplicate registries, caches, seams, compatibility paths, or
  mutable authorities.
- **Fail closed and preserve phases.** Use typed redaction-safe errors and
  explicit checked outcomes. Keep admission, execution, lifecycle, output,
  containment, and cleanup failures distinct. Invalid admission starts no
  child process.
- **Prefer subtraction.** Reuse the existing owner and execution path first.
  Identify the machinery that disappears and estimate the complete production,
  test, and support delta before a simplification change. Compare with a concrete
  smaller alternative; justify added representations, conversions, queries,
  handoffs, and effect phases by guarantees the alternative cannot preserve.
  Remove superseded machinery in the same change. Judge reduction work
  cumulatively against a pinned baseline, including existing branch growth.
- **Prove the guarantee.** Test accepted construction, rejected inputs,
  invariant-preserving transformations, and runtime or relational behavior.
  Reuse or extend existing proofs first; justify added tests and infrastructure
  by distinct guarantees or failure boundaries. Remove superseded proofs only
  when their guarantees remain independently established. Include proof
  maintenance in the design's cost. Never weaken coverage to meet a line target.
  Use compile-time proofs where the language supports them and runtime tests
  for runtime facts; tests support the design but do not replace it.

For every non-trivial change, answer briefly in the discussion or PR:

1. What invariant must always hold?
2. Which invalid states are made unrepresentable?
3. Which owner rejects what the types cannot express, and when?
4. Which existing owner/path is reused, and why is the smaller alternative
   insufficient?
5. What tests or checks prove the invariant and cover coupled surfaces?

## Code Review Rules

- Review the complete feature diff for avoidable machinery; report it as a
  blocking finding even when tests pass. Identify the concrete smaller design
  and the guarantees it preserves. Resolve findings before completion while
  retaining independent regression proofs.
- Before freezing added public fields or diagnostic variants, justify each by
  a concrete consumer need. Keep internal bookkeeping private.
- At completion, report measured added/deleted/net production and test/support
  lines, classifying embedded tests as tests. Account for fixtures, generated
  files, and docs separately, and report the combined total. Moving code between
  categories is not reduction. Justify growth against the smallest complete
  design; never compress code or weaken guarantees to meet a line target.
- For simplification tasks, completion requires lower complexity and fewer
  combined production/test/support lines against a pinned comparison base.
  Report deltas against both that base and starting HEAD, including existing
  branch growth; report an unmet objective if the requirement is not achieved.

## Boundaries

- Use only Nix and Rust as implementation languages, including adapters,
  tooling, tests, and embedded code. Thin shell glue must be embedded and
  packaged in Nix; no standalone `*.sh` files. Bounded command expressions
  (such as `jq`, `sed` and `awk`), storage/query languages (SQL), and declarative
  data/configuration are allowed.
- Nix evaluates, validates, builds, and realises. Rust admits, executes,
  reconciles, and cleans against `manifest.json` and OS reality.
- `manifest.json` is the only required semantic seam; `views/docs.md` is
  disposable. The runtime binary never invokes Nix or imports Nix expressions;
  declared child programs are a separate contract case.
- Keep runtime behavior domain-generic. The manifest remains tasks, services,
  inline invocations, static composite DAGs, and derived graph facts. Do not add
  runtime-owned caches, memberships, or dynamic orchestration.
- Keep runtime-owned host placement and secret values out of the manifest. Child
  environments stay hermetic. Admission, endpoint ownership, state cleanup,
  liveness, containment, and secret handling fail closed.
- Shell and gate scripts may orchestrate tests and assert results, but
  production graph, registry, validation, liveness, summary, and cleanup
  semantics stay in their owning layer. Nix does not supervise or reconcile;
  `nixfied-manifest` contains the shared typed contract, not runtime behavior.
- A new semantic kind, seam, mutable authority, compatibility path, daemon,
  or domain-specific runtime branch requires a contract-consistent design
  review before implementation.

## Authority

Read these before changing the corresponding area:

- User behavior, authoring, and recovery: [`README.md`](README.md),
  [`docs/GUIDE.md`](docs/GUIDE.md), [`docs/ADAPTERS.md`](docs/ADAPTERS.md).
- Manifest, ABI, lifecycle, output, and public surface: [`docs/CONTRACT.md`](docs/CONTRACT.md)
  and [`capability.txt`](runtime/crates/nixfied-manifest/capability.txt).
- Architecture and derived facts: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md),
  [`docs/DERIVATION_SPEC.md`](docs/DERIVATION_SPEC.md).
- Options and generated references: [`docs/OPTIONS.md`](docs/OPTIONS.md).
- Repository layout, tests, gates, and verification: [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md).

## Change routing

| Change | Primary owner | Coupled surfaces |
| --- | --- | --- |
| Declarations, options, endpoints | `nix/modules/`, `nix/adapters/` | `README.md`, `docs/GUIDE.md`, generated `docs/OPTIONS.md` |
| Generated apps and help | `nix/project-apps.nix`, `nix/help-*.nix` | `docs/CONTRACT.md`, app and help tests |
| Resolution, validation, derivation, manifest/docs view | `nix/compiler/` | `docs/DERIVATION_SPEC.md`, Nix vectors and views |
| Manifest fields, ABI, serialization, structural validation | `nix/spec/`, `runtime/crates/nixfied-manifest/` | `capability.txt`, Nix/Rust constants, fixtures, contract tests |
| Admission, lifecycle, execution, and controls | `runtime/crates/nixfied-runtime/` | `docs/CONTRACT.md`, focused runtime tests |
| Install and upgrade behavior | `nix/install/`, `runtime/crates/nixfied-cli/` | `docs/GUIDE.md`, install and package checks |
| Builds, checks, gates, and CI | `nix/checks/`, `nix/gate*.nix`, `nix/dev.nix`, `.github/workflows/` | `docs/DEVELOPMENT.md` |

If a change names multiple participants, treat it as one atomic cutover rather
than separate implementations.

## Contract and delivery

- Treat manifest fields, enums, hidden commands, outputs, errors, and semantics as
  exact ABI. Update `capability.txt`, the ABI snapshot, Nix/Rust constants,
  producers, consumers, tests, fixtures, and docs together. Delete superseded
  readers, aliases, fallbacks, migrations, and dual paths; never reinterpret
  old bytes or rewrite append-only history.
- New manifest fields update their authored declarations and `capability.txt`;
  verify per-record structure and whole-inventory coverage in `nix/checks/`,
  generated freshness, and independent raw-wire/behavioral proofs as mapped in
  `docs/DEVELOPMENT.md`.
  Derived facts update `docs/DERIVATION_SPEC.md`, both derivations, and both
  golden suites. Numeric versions change only when their semantics require it.
- Edit sources, not generated outputs. `nix/compiler/views.nix` renders
  `views/docs.md`; `nix/docs/options.nix` generates `docs/OPTIONS.md`.
- Use the narrowest proof first, then widen according to `docs/DEVELOPMENT.md`.
  `.#test` is fixture-backed, not raw Cargo; use `.#ci` for cross-layer changes
  and `--dirty` when a downstream gate must consume uncommitted changes. Report
  checks run and unverified platform, release, or integration coverage.
- Commit subjects are lower case.
- Divide non-trivial work into ordered logical commits. Each commit must leave one coherent current
