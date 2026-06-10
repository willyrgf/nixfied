# Nixfied

Nixfied is a Nix-authored project model with a generic, Nix-free Rust runtime.
Typed Nix compiles one canonical `model.json`; the runtime admits that model and
starts, inspects, reconciles, cancels, and cleans up its services, tasks, and
workflows.

- **Nix** is the integration + correctness layer — you describe environments,
  services, tasks, workflows, state, ports, and source policy as typed Nix.
- **Rust** is the hidden, generic runtime — it knows no domain and never invokes Nix.
- **`model.json`** is the only semantic seam; `schema` / `docs` / `capabilities`
  are disposable views over it.

The capability line is implemented and shipped: services, tasks, workflow graphs,
multi-slot isolation, the Postgres reference adapter, a polyglot example,
non-destructive `install` / `upgrade`, and a self-hosted conformance gate.

For the design rationale (the *why*) see [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md);
for the contributor contract and invariants see [`AGENTS.md`](AGENTS.md).

## Prerequisites

Nix with flakes enabled — the only requirement. The runtime and CLI build from a
pinned Rust toolchain via `nix` (no host Rust needed). `nix develop` provides the
pinned `cargo` / `clippy` / `rustfmt` for development.

## Quickstart

Build a model (a Nix store output; there is no required `manifest.json`):

```sh
nix build .#minimal-model
# → result/{model.json, views/{schema.json,docs.md,capabilities.json}}
```

Drive it with the runtime (`nixfied-runtime`: `check`, `run`, `ps`, `down`,
`clean`):

```sh
rt="$(nix build .#nixfied-runtime --no-link --print-out-paths)/bin/nixfied-runtime"
model="$(nix build .#minimal-model --no-link --print-out-paths)/model.json"

"$rt" check --model "$model"                                  # validate admission contract
NIXFIED_STATE_DIR=/tmp/nixfied "$rt" run   --model "$model"   # start services, run tasks
NIXFIED_STATE_DIR=/tmp/nixfied "$rt" clean --model "$model"   # marker-gated cleanup
```

A model is admitted only from under the Nix store and only on an exact
`runtimeAbi` / `toolchainId` match. Inspect it through the `nixfied` CLI
(`model` / `schema` / `docs` / `capabilities`):

```sh
nixfied="$(nix build .#nixfied-runtime --no-link --print-out-paths)/bin/nixfied"
"$nixfied" capabilities --model "$model"
```

## Examples

| Example | Shows |
| --- | --- |
| `examples/minimal` | the smallest service + task |
| `examples/postgres` | the Postgres reference adapter (initdb → server → `SELECT 1` → clean) |
| `examples/workflow` | a bounded task DAG with a service-readiness gate |
| `examples/polyglot-stack` | two services in different languages, one run |
| `examples/downstream` | a realistic small system + the copy-paste adoption guide |

Build any via the root flake (e.g. `nix build .#postgres-model`);
`examples/downstream/README.md` walks through adoption.

## Install into another project

```sh
nix run github:willyrgf/nixfied#install   # scaffolds flake.nix + nixfied.nix (won't overwrite)
nix run github:willyrgf/nixfied#upgrade   # repins the nixfied input only
```

## Verify

The gate is the product testing itself — this repo is a Nixfied project whose
`conformance` workflow exercises the framework end to end (the examples,
multi-slot, and a real `install` + `upgrade`), run by the runtime under test. Run
it after any significant change:

```sh
nix run .#gate
```

CI (`.github/workflows/conformance.yml`) runs the full layered gate: the
`cargo test` floor first, then `nix flake check`, then the dogfood workflow. The
exact commands are in [`AGENTS.md`](AGENTS.md).
