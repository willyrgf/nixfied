# Repository Map

Generated from `docs/repo-index.json`.

## Start Here
- `docs/repo-index.json` - Canonical deterministic repository index.
- `docs/repo-map.md` - LLM-facing repository map generated from docs/repo-index.json.
- `README.md` - Primary repository overview and command entrypoints.
- `AGENTS.md` - Agent instructions and collaboration constraints.
- `CLAUDE.md` - Additional assistant guidance for this repository. (missing)
- `ARCHITECTURE.md` - Architecture and system design details. (missing)
- `REDESIGN.md` - Redesign notes and migration context. (missing)

## Components
- `flake.nix` (nix-flake)

## Command Surfaces
- `build` from `nixfied/project/module.nix`
- `check` from `nixfied/project/module.nix`
- `ci` from `nixfied/project/module.nix`
- `dev` from `nixfied/project/module.nix`
- `format` from `nixfied/project/module.nix`
- `framework::install` from `nixfied/project/module.nix`
- `framework::test` from `nixfied/project/module.nix`
- `test` from `nixfied/project/module.nix`

## Sensitive Zones
- `nixfied/.framework` - Framework internals; avoid direct edits in installed repos. (checks: nix run .#help)
- `nixfied/project/module.nix` - Primary v2 command/task/workflow surface. (checks: nix run .#help, nix run .#framework::test, nix run .#ci -- --summary)
- `nixfied/project/conf.nix` - Project identity, environment names, and port contract. (checks: nix run .#validate-env, nix run .#ci -- --summary)

## Canonical Commands
- `nix run .#help`
- `nix run .#dev`
- `nix run .#test`
- `nix run .#framework::test`
- `nix run .#build`
- `nix run .#check`
- `nix run .#ci -- --summary`

## Invariants
- Treat `nixfied/project/` as the primary customization surface.
- Keep command metadata (`api`) aligned with script behavior.
- Keep this map and `docs/repo-index.json` in sync via `nix run .#check`.
