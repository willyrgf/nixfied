# Nixfied

Nixfied lets a project describe its services, tasks, and exported workflows
once in typed Nix. Nix evaluates and builds that declaration into one canonical
`model.json`; a generic, Nix-free Rust runtime then owns process execution,
ports, state, reconciliation, and cleanup.

The model has two kinds:

- a **task** is a bounded invocation or a static composite DAG;
- a **service** is a runtime-owned long-lived process with lifecycle and
  optional endpoint declarations.

Projects name those primitives in their own vocabulary—`lint`, `database`,
`dev`, `test`, `release`—without adding domain logic to the runtime. Several
isolated slots can run side by side with separate ports, processes, and state.

## Quick start

The only prerequisite is Nix with flakes enabled. Inspect Nixfied's own
runnable apps without a checkout:

```sh
nix run github:willyrgf/nixfied#help
```

The catalog is reference-neutral. Reuse the same flake reference with a listed
name, for example `nix run github:willyrgf/nixfied#check`.

In a project that does not already have a `flake.nix`, run:

```sh
nix run github:willyrgf/nixfied#install -- \
  --project-id my-project \
  --name "My Project"
```

The installer creates:

- `flake.nix`, which declares Nixfied and exposes the compiled model and
  generated apps;
- `nixfied.nix`, which the project owns and edits.

It starts with a synthetic service and exported `smoke` task, so the complete
path is immediately runnable. In a Git worktree, stage `flake.nix` and
`nixfied.nix` before locking, then commit them with the generated `flake.lock`:

```sh
git add flake.nix nixfied.nix  # Git worktrees only: expose new sources to Nix
nix flake lock                 # pin the declared inputs
git add flake.lock             # Git worktrees only: include the pin in the commit
nix run .#help              # list every runnable command and its purpose
nix build .#model           # evaluate, validate, and compile the model
nix run .#model-check       # admit it without executing a task
nix run .#smoke             # run the starter's exported task
```

Replace the starter declarations with your services and workflows. Existing
flakes are supported through the same `compileModel` and `projectApps` calls;
the installer refuses to edit an existing `flake.nix` and prints the merge
fragment instead.

The distribution boundary is deliberate: `.#install` is a CLI-only scaffold
wrapper and does not depend on the process runtime. Generated project apps
(`run`, `model-check`, `ps`, `down`, `clean`, and exported task verbs) use the
release `nixfied-runtime` package. The debug runtime and test child are private
framework-check inputs, not adopter-facing package outputs.

See the [adopter guide](docs/GUIDE.md) for existing-flake integration, the
authoring model, command semantics, slots and state, secrets, adapters, and
upgrades.

## How the boundary works

- **Nix** is the public integration and correctness layer. Typed modules reject
  invalid intent and realise every executable closure.
- **Rust** is the hidden impure runtime. It admits the compiled model, starts and
  reconciles process groups, verifies endpoint ownership, and safely cleans
  runtime-owned state. It never invokes Nix.
- **`model.json`** is the only semantic seam. Its `views/docs.md` file is a
  disposable, model-derived human reference.

### Ownership in practice

Nixfied owns:

- task and service execution;
- process containment and cancellation;
- service state and lifecycle;
- declared endpoint placement and ownership verification;
- runtime-owned run evidence;
- marker-gated, path-confined cleanup of state roots it owns.

Compiler and tool caches remain child-, tool-, and project-owned. Nixfied does
not own their:

- placement or compatibility policy;
- writer arbitration;
- inventory, timestamps, or byte accounting;
- retention or eviction policy;
- corruption recovery;
- cold/warm modes;
- operator history.

Build the model to inspect the exact tasks, services, slots, port windows, and
state policy of an adopted project:

```sh
nix build .#model
less result/views/docs.md
```

## Documentation

- [Adopter guide](docs/GUIDE.md) — installation, integration, authoring,
  operations, upgrades, and project documentation boundaries.
- [Option reference](docs/OPTIONS.md) — generated `nixfied.nix` option types,
  defaults, and descriptions.
- [Adapter guide](docs/ADAPTERS.md) — adapter authoring and endpoint
  conventions.
- [Contract](docs/CONTRACT.md) — normative model, lifecycle, state, and output
  behavior.
- [Architecture](docs/ARCHITECTURE.md) — rationale and product boundaries.
- [Development guide](docs/DEVELOPMENT.md) — repository layout, checks, gates,
  and test placement.
- [Derivation specification](docs/DERIVATION_SPEC.md) — canonical graph facts
  derived independently by Nix and Rust.

## Examples

| Example | Demonstrates |
| --- | --- |
| [`minimal`](examples/minimal) | Smallest service and task |
| [`postgres`](examples/postgres) | Postgres adapter lifecycle and smoke query |
| [`reth`](examples/reth) | Multi-endpoint Reth adapter and JSON-RPC probe |
| [`composite`](examples/composite) | Static task DAG over a service |
| [`polyglot-stack`](examples/polyglot-stack) | Two language-specific services in one check |
| [`downstream`](examples/downstream) | Realistic database, API, worker, and release flow |
| [`toolchain`](examples/toolchain) | Heterogeneous tools, nested composites, and an endpoint-less worker |

For example:

```sh
nix build --no-write-lock-file ./examples/downstream#model
```

## Developing Nixfied

Run `nix run .#help` to list the framework repository commands. The canonical
local repository gate is:

```sh
nix run .#ci
```

It runs the source and build checks, white-box Cargo tests, and both integration
gates. Use the [development guide](docs/DEVELOPMENT.md) for focused checks and
the exact coverage of each stage.
