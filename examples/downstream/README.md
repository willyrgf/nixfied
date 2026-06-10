# Downstream Worked Example

A realistic "small system" you can copy as the starting point for adopting
Nixfied: a Postgres database, a first-party `api` service, a `worker` service,
and a `release` workflow that ties them together. The framework gate installs and
drives this example (among the others) to prove the framework works end to end.

Everything is declared as typed Nix that compiles into the generic model
primitives. The Rust runtime knows nothing about Postgres, the api, or the
worker — it executes `model.json`.

## Layout

| File | Owner | Purpose |
| --- | --- | --- |
| `nixfied.nix` | you | every semantic declaration: services, tasks, workflow, slots, ports |
| `flake.nix` | Nixfied wiring | pins the `nixfied` input and exposes `packages.<system>.model` |

The split is the install/upgrade ownership boundary: `nixfied install` scaffolds
`flake.nix`, `nixfied upgrade` repins the input, and neither command ever
touches your `nixfied.nix`.

## What it declares

- **Database** — the reusable `adapters.postgres` Nix-side adapter contributes a
  `postgres` service (initdb → server → TCP-ownership readiness → health → stop →
  marker-gated clean) and a `smoke-query` task (`SELECT 1`).
- **Services** — `api` and `worker`, two foreground TCP services built from a
  small Nix-packaged executable. Real projects point closures at their own
  binaries instead.
- **Tasks** — `ping-api` and `ping-worker`, each gated on its service's
  readiness, plus the adapter's `smoke-query`.
- **Workflow** — `release`: `db-check` (the `SELECT 1`) runs first, then
  `api-check` and `worker-check` run once the database has answered.
- **Slots** — `slotPolicy.max = 1`, so two slots of the same environment can run
  side by side with disjoint ports, state, and registries.

## Build and run

```sh
# Compile the model (a Nix store output).
nix build ./examples/downstream#model
model="$(nix build ./examples/downstream#model --no-link --print-out-paths)/model.json"

# Build the runtime from this repo (host-Rust-free).
runtime="$(nix build .#nixfied-runtime --no-link --print-out-paths)/bin/nixfied-runtime"

# Validate the admission contract without starting anything.
"$runtime" check --model "$model"

# Start every service, run every task, then clean.
NIXFIED_STATE_DIR=/tmp/downstream-state "$runtime" run --model "$model"
NIXFIED_STATE_DIR=/tmp/downstream-state "$runtime" clean --model "$model"

# Or drive the release workflow.
NIXFIED_STATE_DIR=/tmp/downstream-state "$runtime" run --model "$model" --workflow release
NIXFIED_STATE_DIR=/tmp/downstream-state "$runtime" clean --model "$model"
```

`run` prints a JSON summary (services, per-task success, the computed model
hash, and — for a workflow — the ordered node results). `clean` is marker-gated
and path-confined: it only removes state roots this model owns.

### With the generated apps

This example's `flake.nix` wires `nixfied.lib.<system>.projectApps`, so the same
operations are one command each — the surface every nixfied project gets:

```sh
nix run ./examples/downstream#check                      # admission only
nix run ./examples/downstream#run                        # start services, run tasks
nix run ./examples/downstream#run -- --workflow release  # drive the release workflow
```

`.#test` / `.#ci` run a project's `test` workflow; this example ships a `release`
workflow instead (its acceptance gate), so it is driven via `.#run -- --workflow
release`. A fresh `#install` scaffold ships a `test` workflow by default. State
goes to the default location (`$XDG_STATE_HOME/nixfied`); set `NIXFIED_STATE_DIR`
to override.

## macOS + Linux

The example runs on both. Postgres is configured TCP-only (the Unix socket is
disabled in `adapters.postgres`) to avoid the macOS `sun_path` length limit
under deep state directories.
