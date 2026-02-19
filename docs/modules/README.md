# Module Documentation

These pages document module configuration knobs in `nixfied/project/conf.nix`.

The current framework surface does not expose dedicated module command namespaces. Module behavior is consumed through shared tasks/workflows and model-derived operations.

Use `nix run .#help` for the live app list.

## Configuration Source of Truth

- Module enablement and defaults: `nixfied/project/conf.nix`
- Service option schemas: `nixfied/modules/services/*.nix`
- Project task/workflow wiring: `nixfied/project/module.nix`

## Operational Validation

- `nix run .#validate-env` checks slot/env constraints.
- `nix run .#ports` prints computed per-slot/per-env ports.
- `nix run .#check-ports` reports current listener status.

## Module Pages

- `docs/modules/postgres.md`
- `docs/modules/nginx.md`
- `docs/modules/minio.md`
- `docs/modules/reth.md`
- `docs/modules/helios.md`
