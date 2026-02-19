# Nixfied (Model-First v2)

Nixfied is a model-driven Nix framework.

The canonical source of truth is `nixfiedModel.v2`, compiled from typed modules and exposed through stable flake interfaces.

## Quick Start

```bash
nix run .#help
nix run .#dev
nix run .#test
nix run .#ci -- --mode full --summary
```

## Documentation

- `ARCHITECTURE.md` - High-level v2 architecture reference.
- `docs/DETAILED.md` - Detailed model/runtime/registry contracts.
- `REDESIGN.md` - v2 redesign and migration context.

## Core Architecture

- Module system: `lib.evalModules` with typed options (`nixfied/modules/*.nix`).
- Compiler pipeline: explicit passes (`nixfied/compiler/*.nix`).
- Canonical hashing: `stateHash = sha256(toCanonicalNix(model))`.
- Runner: single dispatcher with:
  - `run-task <task-id> [-- ...]`
  - `run-workflow <workflow-id> [-- ...]`
- Registry: strict NDJSON event log and deterministic replay.

## Stable Flake Interfaces

Introspection outputs:

- `nix run .#model`
- `nix run .#stateHash`
- `nix run .#tasks`
- `nix run .#task::<id>`
- `nix run .#schema`

Operational outputs:

- `nix run .#validate-env`
- `nix run .#test-isolation`
- `nix run .#ports`
- `nix run .#check-ports`

## API

`nixfied.lib.mkNixfied` is the primary entrypoint:

```nix
nixfied.lib.mkNixfied {
  system = "x86_64-linux";
  projectRoot = ./.;
  projectModules = [ ./nixfied/project/module.nix ];
  extraModules = [ ];
}
```

Return shape:

```nix
{
  model;
  stateHash;
  tasks;
  workflows;
  apps;
  packages;
  checks;
  devShells;
}
```

## Schemas

Published external schemas:

- `nixfied/schemas/task-contract-v1.json`
- `nixfied/schemas/workflow-contract-v1.json`
- `nixfied/schemas/model-export-v1.json`

## Installer

- `nix run .#framework::install` creates a thin wrapper flake.
- `nix run .#framework::install -- --vendor` creates a vendored wrapper layout.

## Determinism Gates

See `tests/framework/v2/` for model hash, scheduler order, help snapshot, and registry replay gates.
