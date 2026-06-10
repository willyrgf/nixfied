# Nixfied

Nixfied is a Nix-authored project model with a generic Rust runtime for owned
execution of project-shaped systems. Typed Nix evaluates to one canonical
`model.json`; a single, Nix-free Rust runtime admits that model and starts,
inspects, reconciles, cancels, and cleans up its services, tasks, and workflows.

One boundary defines the product:

- **Nix is the user-facing integration and correctness layer.** You describe
  environments, services, tasks, workflows, state, ports, and source policy as
  typed Nix; invalid intent never compiles into an admitted model.
- **Rust is the hidden, generic execution runtime.** It knows nothing about
  Postgres, Python, or any domain. It executes the generic primitives Nix
  compiles into `model.json`, enforcing typed lifecycle semantics, and it never
  invokes Nix.
- **`model.json` is the only semantic seam.** `schema`, `docs`, and
  `capabilities` are disposable views over it — never independent authority.

## Status

The capability line is implemented and shipped:

- one typed Nix module → one Nix-store `model.json` with a full admission
  contract (ABI/toolchain, target, source/codebase, closures, state, secrets);
- a generic runtime: admission, per-slot SQLite registry, state placement,
  foreground services with endpoint-ownership-verified readiness, bounded tasks,
  workflow graphs, cancellation, OS-reconciled liveness, and marker-gated
  cleanup;
- the Postgres reference adapter and a polyglot example, both pure Nix-side model
  generators (no service-specific runtime code);
- multi-slot isolation; multiple services per run;
- non-destructive `install` / `upgrade` adoption surfaces;
- a self-hosted conformance gate: this repo is itself a Nixfied project whose
  `conformance` workflow exercises the framework end to end, run by the runtime
  under test.

See [Deferred](#deferred) for what is intentionally not built yet.

## How it works

```text
typed Nix modules / your flake
      │  resolve → validate → derive → emit
      ▼
Nix store output
      model.json          # the only required semantic seam
      views/{schema.json,docs.md,capabilities.json}   # disposable projections
      realised closures   # referenced by store path
      ▼
nixfied-runtime (Nix-free)
      load + sha256 raw bytes → computedModelHash
      admission: store origin, exact ABI/toolchain, target, source, closures, state, secrets
      execute: services, tasks, workflows, probes, process groups
      reconcile + clean
      ▼
per-slot SQLite registry, state roots, logs, artifacts, summaries
```

Two binaries, by verb:

- **`nixfied-runtime`** — the hidden engine. Consumes a `model.json` by path and
  performs all impure execution: `check`, `run` (task or `--workflow`), `ps`,
  `down`, `clean`. It never calls Nix.
- **`nixfied`** — the ergonomic CLI for model views and adoption: `model`,
  `schema`, `docs`, `capabilities`, `install`.

`compile` is `nix build .#<model>`; adoption (`install` / `upgrade`) and the
conformance gate are also exposed as flake apps.

## Repository layout

```text
flake.nix                       packages, apps, checks, dev shell
nix/
  modules/                      typed user-facing declaration surface
  compiler/                     resolve → validate → derive → emit model + views
  spec/                         contract constants and model shape
  adapters/                     Nix-side adapters (synthetic, postgres) + default.nix
  install/                      install/upgrade surfaces (shell embedded in .nix)
  packages/                     host-Rust-free build of the runtime binaries
  gate.nix                      `nix run .#gate` launcher for the conformance gate
  lib/                          pure Nix helpers
nixfied.nix                     the framework's self-project: the conformance workflow
runtime/crates/
  nixfied-model                 serde model contract + structural validation
  nixfied-runtime               Nix-free admission, registry, state, services, tasks, workflows, controls
  nixfied-cli                   ergonomic CLI: model/schema/docs/capabilities/install
  nixfied-conformance           per-check conformance closure (the workflow's task nodes)
examples/                       minimal, postgres, workflow, polyglot-stack, downstream
```

## Prerequisites

- Nix with flakes enabled.

That is the only requirement. The runtime and CLI are built from a pinned Rust
toolchain via `nix`; you do not need a host Rust toolchain to use Nixfied. For
development, `nix develop` provides the pinned `cargo`/`clippy`/`rustfmt`.

## Quickstart

Build a model (a Nix store output):

```sh
nix build .#minimal-model
# → result/{model.json, views/{schema.json,docs.md,capabilities.json}}
```

There is intentionally no required `manifest.json`.

Build the runtime and drive the model:

```sh
rt="$(nix build .#nixfied-runtime --no-link --print-out-paths)/bin/nixfied-runtime"
model="$(nix build .#minimal-model --no-link --print-out-paths)/model.json"

"$rt" check --model "$model"                       # validate the admission contract
NIXFIED_STATE_DIR=/tmp/nixfied "$rt" run   --model "$model"   # start services, run tasks
NIXFIED_STATE_DIR=/tmp/nixfied "$rt" ps    --model "$model"   # OS-reconciled liveness
NIXFIED_STATE_DIR=/tmp/nixfied "$rt" clean --model "$model"   # marker-gated cleanup
```

A `model.json` is admitted only from under the Nix store and only when its
`runtimeAbi` / `toolchainId` exactly match the runtime. `NIXFIED_STATE_DIR`
selects the state base (otherwise a documented platform default is used).

Inspect the model through generated views:

```sh
nixfied="$(nix build .#nixfied-runtime --no-link --print-out-paths)/bin/nixfied"
"$nixfied" capabilities --model "$model"   # agent-readable catalog
"$nixfied" docs         --model "$model"   # human-readable docs
```

## Examples

| Example | Shows |
| --- | --- |
| `examples/minimal` | the smallest service + task |
| `examples/postgres` | the Postgres reference adapter (initdb → server → `SELECT 1` → clean) |
| `examples/workflow` | a bounded task DAG with a service-readiness gate |
| `examples/polyglot-stack` | two services in different languages, one run |
| `examples/downstream` | a realistic small system (Postgres + api + worker + a `release` workflow + multi-slot) and the adoption guide |

Each builds via the root flake (e.g. `nix build .#postgres-model`) or through its
own `flake.nix`. `examples/downstream/README.md` is the copy-paste adoption guide.

## Install into another project

```sh
nix run github:willyrgf/nixfied#install      # scaffolds flake.nix + nixfied.nix
nix run github:willyrgf/nixfied#upgrade      # repins the nixfied input only
```

`install` creates `flake.nix` and `nixfied.nix` only when absent and refuses to
edit an existing `flake.nix`. `upgrade` rewrites only the `nixfied` input pin and
lock entry; it never touches your project-owned `nixfied.nix`. Old compiled
models are not promised to run on new runtimes — recompile after upgrading.

## Verification

The end-to-end gate is the product testing itself: this repo is a Nixfied
project whose `conformance` workflow drives the framework through its own
surfaces. It is layered — a trusted `cargo test` floor, then `nix flake check`
structural gates, then the dogfood workflow run by the nix-built runtime.

```sh
# 1. Trusted floor (pinned toolchain via the dev shell).
nix develop --command bash -c 'cd runtime && cargo fmt --all -- --check'
nix develop --command bash -c 'cd runtime && cargo clippy --workspace --all-targets --all-features -- -D warnings'
nix develop --command bash -c 'cd runtime && cargo test --workspace'

# 2. Structural gate: every model + the self-model build, the binaries compile.
nix flake check

# 3. Dogfood gate: nixfied runs its own conformance workflow. Run from the repo
#    root after any significant change; forward args after `--`.
nix run .#gate                    # or: nix run .#gate -- --timeout-ms 120000
```

`nix run .#gate` rebuilds the runtime + self-model from the working tree,
smoke-checks, then runs the workflow in a fixed state dir (`$TMPDIR/nixfied-gate`)
wiped fresh each run and kept afterward so the reported summary, logs, and
ground-truth verdicts stay inspectable. `.github/workflows/conformance.yml` runs
the same layers in CI.

The conformance workflow's nodes are per-check closures: capability checks drive
each example model (`minimal`, `workflow`, `polyglot`, `postgres`, `downstream`)
and `slots` through the nix-built runtime; `adoption` runs the real `#install` +
`#upgrade` against a throwaway repo; `negative` proves the gate fails closed.
There are no e2e shell scripts — lifecycle/cancellation/GC invariants and SEAM-1
(the runtime never invokes Nix) are white-box cargo tests.

## Guarantees

- **Single seam.** `model.json` is the only required semantic artifact; views are
  disposable projections.
- **No Nix in the runtime.** `nixfied-runtime` never invokes `nix`, `nix-store`,
  `nix build`, or `nix eval`; closures are realised by Nix before it starts.
- **Generic runtime.** Concrete services (e.g. Postgres) are Nix-side model
  generators; the runtime has no service-specific code paths.
- **Exact contract.** A model is admitted only under the Nix store and only on an
  exact `runtimeAbi` / `toolchainId` match. No cross-version compatibility.
- **Owned execution.** Every process belongs to a runtime-owned process group;
  liveness is reconciled against the OS; readiness requires verified endpoint
  ownership; cleanup is marker-gated, path-confined, and crash-safe.

## Deferred

Intentionally not built yet (no compatibility promise either way):

- the ergonomic surfaces `up`, `logs`, and `validate --deep`;
- multiple environments beyond `dev`;
- `until-idle` / `persistent-until-down` service lifetimes, service reuse, and
  borrower leases (only `run-scoped` is implemented);
- additional source modes (`snapshot`, `flake-input`) and non-`fail` port
  collision policies;
- real secret injection (the model accepts only an empty `secrets` section);
- a portable manifest envelope and a dynamic runtime adapter protocol.

`AGENTS.md` is the contributor guide and records the architecture, invariants,
and boundaries to honor when extending the framework.
