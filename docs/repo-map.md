# Repository Map

Generated from `docs/repo-index.json`.

## Start Here
- `docs/repo-index.json` - Canonical deterministic repository index.
- `docs/repo-map.md` - LLM-facing repository map generated from docs/repo-index.json.
- `README.md` - Primary repository overview and command entrypoints.
- `docs/DETAILED.md` - Detailed model architecture and contracts.
- `ARCHITECTURE.md` - High-level architecture reference.
- `REDESIGN.md` - Redesign and migration context.
- `AGENTS.md` - Agent instructions and collaboration constraints.
- `CLAUDE.md` - Additional assistant guidance for this repository.

## Components
- `flake.nix` (nix-flake)

## Command Surfaces
- `build` from `nixfied/project/module.nix`
- `check` from `nixfied/project/module.nix`
- `check-ports` from `nixfied/project/module.nix`
- `ci` from `nixfied/project/module.nix`
- `dev` from `nixfied/project/module.nix`
- `format` from `nixfied/project/module.nix`
- `framework::install` from `nixfied/project/module.nix`
- `framework::test` from `nixfied/project/module.nix`
- `ports` from `nixfied/project/module.nix`
- `test` from `nixfied/project/module.nix`
- `test-isolation` from `nixfied/project/module.nix`
- `validate-env` from `nixfied/project/module.nix`

## Sensitive Zones
- `nixfied/.framework` - Framework install internals and workspace marker handling. (checks: nix run .#help)
- `nixfied/project/module.nix` - Primary task/workflow/app surface. (checks: nix run .#help, nix run .#framework::test, nix run .#ci -- --summary)
- `nixfied/project/conf.nix` - Project identity, env names, and port contract. (checks: nix run .#validate-env, nix run .#ci -- --summary)

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

## Invariants
- Treat `nixfied/project/` as the primary customization surface.
- Keep `nixfied/project/module.nix` task/workflow metadata aligned with script behavior.
- Keep this map and `docs/repo-index.json` in sync when command surfaces or key docs change.
