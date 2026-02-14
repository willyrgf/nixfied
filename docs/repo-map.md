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
- `build` from `nixfied/project/prod.nix`
- `check` from `nixfied/project/quality.nix`
- `ci` from `nixfied/project/ci.nix`
- `dev` from `nixfied/project/dev.nix`
- `format` from `nixfied/project/format.nix`
- `test` from `nixfied/project/test.nix`

## Sensitive Zones
- `nixfied/.framework` - Framework internals; avoid direct edits in installed repos. (checks: nix run .#help)
- `nixfied/project/ci.nix` - CI pipeline behavior and release gates. (checks: nix run .#ci -- --summary)
- `nixfied/project/conf.nix` - Project identity, environment names, and port contract. (checks: nix run .#check, nix run .#ci -- --summary)
- `nixfied/project/quality.nix` - Quality checks and discovery drift enforcement. (checks: nix run .#check)

## Canonical Commands
- `nix run .#help`
- `nix run .#dev`
- `nix run .#test`
- `nix run .#build`
- `nix run .#check`
- `nix run .#ci -- --summary`

## Invariants
- Treat `nixfied/project/` as the primary customization surface.
- Keep command metadata (`api`) aligned with script behavior.
- Keep this map and `docs/repo-index.json` in sync via `nix run .#check`.
