# Downstream Worked Example

A realistic "small system" you can copy as the starting point for adopting
Nixfied: a Postgres database, a first-party `api` service, a `worker` service,
and a `release` composite task that ties them together. The framework gate installs and
drives this example (among the others) to prove the framework works end to end.

Everything is declared as typed Nix that compiles into the generic model
primitives. The Rust runtime knows nothing about Postgres, the api, or the
worker — it executes `model.json`.

## Layout

| File | Owner | Purpose |
| --- | --- | --- |
| `nixfied.nix` | you | every semantic declaration: services, tasks, composites, verbs, slots, ports |
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
- **Tasks** — `ping-api` and `ping-worker`, each requiring its service ready
  while it runs, plus the adapter's `smoke-query` (referenced as a step — the
  converse-reuse pattern that replaced environment membership).
- **Composite** — `release`: `db-check` (the `SELECT 1`) runs first, then
  `api-check` and `worker-check` once the database has answered, then the
  `gate` over the whole stack. Running it brings up exactly the services its
  leaves require — `servicesRequired` is derived, never curated.
- **Verbs** — `nixfied.surface.verbs = [ "release" ]` exports the composite as
  the project's one public flake app.
- **Slots** — `slotPolicy.max = 1`, so two slots can run side by side with
  disjoint ports, state, and registries.

## Build and run

```sh
# Compile the model (a Nix store output).
nix build ./examples/downstream#model
model="$(nix build ./examples/downstream#model --no-link --print-out-paths)/model.json"

# Build the runtime from this repo (host-Rust-free).
runtime="$(nix build .#nixfied-runtime --no-link --print-out-paths)/bin/nixfied-runtime"

# Validate the admission contract without starting anything.
"$runtime" check --model "$model"

# Run the release composite: it starts the derived service union (postgres,
# api, worker), runs the flattened steps, then stops everything.
NIXFIED_STATE_DIR=/tmp/downstream-state "$runtime" run --model "$model" --task release
NIXFIED_STATE_DIR=/tmp/downstream-state "$runtime" clean --model "$model"

# `run` with no --task refuses and lists the declared tasks.
```

`run` prints a JSON summary (services, per-node success keyed by step path,
the computed model hash). `clean` is marker-gated and path-confined: it only
removes state roots this model owns.

### With the generated apps

This example's `flake.nix` wires `nixfied.lib.<system>.projectApps`, so the same
operations are one command each — the surface every nixfied project gets:

```sh
nix run ./examples/downstream#admit                   # admission only
nix run ./examples/downstream#release                 # the exported verb
nix run ./examples/downstream#run -- --task ping-api  # any declared task
```

The control namespace (`run`/`ps`/`down`/`clean`/`admit`) is framework-owned;
`release` is this project's exported verb (`nixfied.surface.verbs`). State
goes to the default location (`$XDG_STATE_HOME/nixfied`); set `NIXFIED_STATE_DIR`
to override.

## macOS + Linux

The example runs on both. Postgres is configured TCP-only (the Unix socket is
disabled in `adapters.postgres`) to avoid the macOS `sun_path` length limit
under deep state directories.
