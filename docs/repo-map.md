# Repository Map

Generated from `docs/repo-index.json`.

## Start Here
- `docs/repo-index.json` - Canonical deterministic repository index.
- `docs/repo-map.md` - LLM-facing repository map generated from docs/repo-index.json.
- `README.md` - Primary repository overview and command entrypoints.
- `CLEANUPS.md` - Current repository cleanup ledger and maintenance queue. (missing)
- `docs/DETAILED.md` - Detailed model architecture and contracts.
- `docs/UPGRADE.md` - Downstream upgrade notes for behavioral and path contract changes.
- `docs/ARCHITECTURE.md` - High-level architecture reference.
- `AGENTS.md` - Agent instructions and collaboration constraints.

## Components
- `flake.nix` (nix-flake)

## Command Surfaces
- `build` from `nixfied/project/module.nix`
- `check` from `nixfied/project/module.nix`
- `check-ports` from `nixfied/modules/operations.nix`
- `ci` from `nixfied/project/module.nix`
- `dev` from `nixfied/project/module.nix`
- `format` from `nixfied/project/module.nix`
- `framework::install` from `nixfied/project/module.nix`
- `framework::test` from `nixfied/project/module.nix`
- `framework::upgrade` from `nixfied/project/module.nix`
- `health` from `nixfied/modules/operations.nix`
- `ports` from `nixfied/modules/operations.nix`
- `ready` from `nixfied/modules/operations.nix`
- `test` from `nixfied/project/module.nix`
- `test-isolation` from `nixfied/modules/operations.nix`
- `validate-env` from `nixfied/modules/operations.nix`

## Dispatcher and Introspection
- `run-task -- <task-id> [-- ...]` from `nixfied/runner/dispatcher.nix`
- `run-workflow -- <workflow-id> [-- ...]` from `nixfied/runner/dispatcher.nix`
- `run-workflow-parallel -- <workflow-id> [-- ...]` from `nixfied/runner/dispatcher.nix`
- `runs [run-id]` from `nixfied/runner/dispatcher.nix`
- `stop-run -- <run-id>` from `nixfied/runner/dispatcher.nix`
- `stop-all-runs` from `nixfied/runner/dispatcher.nix`
- `model`, `stateHash`, `tasks`, `services`, `task::<id>`, `schema` from `nixfied/lib/mkNixfied.nix`

## Sensitive Zones
- `nixfied/.framework` - Framework internals; avoid direct edits in installed repos. (checks: nix run .#help)
- `nixfied/project/conf.nix` - Project identity, environment names, and port contract. (checks: nix run .#validate-env, nix run .#ci -- --summary)
- `nixfied/project/module.nix` - Primary command/task/workflow surface. (checks: nix run .#help, nix run .#framework::test, nix run .#ci -- --summary)

## Canonical Commands
- `nix run .#help`
- `nix run .#dev`
- `nix run .#test`
- `nix run .#build`
- `nix run .#check`
- `nix run .#format`
- `nix run .#ci -- --summary`
- `nix run .#validate-env`
- `nix run .#test-isolation`
- `nix run .#ports`
- `nix run .#check-ports`
- `nix run .#framework::test`
- `nix run .#framework::install`
- `nix run .#framework::upgrade`

## Invariants
- Treat `nixfied/project/` as the primary customization surface.
- Keep command metadata aligned with script behavior.
- Keep this map and `docs/repo-index.json` in sync when command surfaces or key docs change.
