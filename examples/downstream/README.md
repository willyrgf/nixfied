# Downstream Worked Example

A realistic "small system" you can copy as the starting point for adopting
Nixfied: a Postgres database, a first-party `api` service, a `worker` service,
and a `release` composite task that ties them together. The framework gate installs and
drives this example (among the others) to prove the framework works end to end.

Everything is declared as typed Nix that compiles into the generic manifest
primitives. The Rust runtime knows nothing about Postgres, the api, or the
worker — it executes `manifest.json`.

## Layout

| File | Owner | Purpose |
| --- | --- | --- |
| `nixfied.nix` | you | every semantic declaration: services, tasks, composites, verbs, slots, ports |
| `flake.nix` | example wiring | declares the `nixfied` input and exposes `packages.<system>.manifest` |

The split keeps reusable project declarations separate from this repository's
example-only flake wiring.

## What it declares

The example combines a reusable database adapter with first-party API and
worker services. Its release workflow composes adapter checks and local tasks;
running it brings up exactly the services its leaves require. Real projects
replace the small packaged service executable with their own binaries.

The workflow first verifies that the database answers, then checks the API and
worker after that verification succeeds. Once both checks pass, a final gate
exercises the whole stack. These step dependencies express the release order;
each leaf's service requirements determine what must be ready while it runs.
The adapter's check participates because the composite explicitly references it,
so the project can reuse the adapter without adopting a separate workflow.

Read the exact tasks, services, derived requirements, composite steps and slot
windows from the compiled example rather than a separately maintained list:

```sh
nix build --no-write-lock-file ./examples/downstream#manifest
less result/views/docs.md
```

The declaration exports the release workflow through `nixfied.surface.verbs`
when an adopter wires `projectApps`. For that framework interface, use
`nix run .#docs -- option nixfied.surface.verbs` from the framework checkout.

## Build and run

```sh
# Compile the manifest (a Nix store output).
nix build --no-write-lock-file ./examples/downstream#manifest
manifest="$(nix build --no-write-lock-file ./examples/downstream#manifest --no-link --print-out-paths)/manifest.json"

# Build the runtime from this repo (host-Rust-free).
runtime="$(nix build .#nixfied-runtime --no-link --print-out-paths)/bin/nixfied-runtime"

# Validate the admission contract without starting anything.
"$runtime" check --manifest "$manifest"

# Run the release composite: it starts the derived service union (postgres,
# api, worker), runs the flattened steps, then stops everything.
NIXFIED_STATE_DIR=/tmp/downstream-state "$runtime" run --manifest "$manifest" --task release
NIXFIED_STATE_DIR=/tmp/downstream-state "$runtime" clean --manifest "$manifest"

# `run` with no --task refuses and lists the declared tasks.
```

State goes to the default location (`$XDG_STATE_HOME/nixfied`); set
`NIXFIED_STATE_DIR` to override.

## macOS + Linux

The example runs on both. Postgres is configured TCP-only (the Unix socket is
disabled in `adapters.postgres`) to avoid the macOS `sun_path` length limit
under deep state directories.
