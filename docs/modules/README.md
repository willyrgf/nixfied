# Module Documentation

These pages track module configuration notes only.

Current command surfaces are model-generated from `nixfied/project/module.nix` and do not expose dedicated module app namespaces.
Use `nix run .#help` for the live command list.

## Configuration source

- `nixfied/project/conf.nix` controls module enablement and module-specific port keys.
- Runtime validation for env/slot settings is provided by `nix run .#validate-env`.

## Module pages

- `docs/modules/postgres.md`
- `docs/modules/nginx.md`
- `docs/modules/minio.md`
- `docs/modules/reth.md`
- `docs/modules/helios.md`
