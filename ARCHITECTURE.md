# Architecture (v2)

Nixfied is a model-first framework. Typed Nix modules compile into `nixfiedModel.v2`, and command/help/docs surfaces are generated views of that model.

## Pipeline

`modules -> resolved config -> compiler passes -> nixfiedModel.v2 -> stateHash + apps + docs`

Compiler passes:
1. `resolve-modules`
2. `normalize-runtime`
3. `compile-services`
4. `compile-tasks`
5. `compile-workflows`
6. `compile-views`
7. `finalize-model`

## Core Directories

- `nixfied/project/`: project-owned config (`conf.nix`, `module.nix`).
- `nixfied/modules/`: typed option definitions.
- `nixfied/compiler/`: deterministic model compilation passes.
- `nixfied/runner/`: dispatcher and executor runtime.
- `nixfied/registry/`: strict NDJSON event store, snapshot, replay.
- `nixfied/lib/`: canonical rendering and `mkNixfied` entrypoint.

## Runtime Contract

- Single dispatcher surfaces: `run-task` and `run-workflow`.
- Hermetic task boundary with deterministic defaults (`LANG`, `LC_ALL`, `TZ`, `umask`, `cwd` policy).
- Stable, prefix-based CLI output (`INFO:`, `WARN:`, `ERROR:`, `OK:`, `SKIP:`).
- Deterministic scheduling and lock arbitration.

## Flake Surfaces

- User-facing workflows/apps: `help`, `dev`, `test`, `build`, `check`, `format`, `ci`, `validate-env`, `test-isolation`, `ports`, `check-ports`, `framework::test`, `framework::install`.
- Introspection apps: `model`, `stateHash`, `tasks`, `task::<id>`, `schema`.

## Determinism Gates

Authoritative checks are in `tests/framework/v2/` and include:
- model hash stability
- cross-machine hash stability
- scheduler order determinism
- help snapshot contract
- registry replay/events contract
- executor and env-sandbox contracts
