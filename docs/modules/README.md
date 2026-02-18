# Module Documentation

This directory contains detailed docs for Nixfied optional service modules.

## Shared module contract

- Enable modules in `nixfied/project/conf.nix` under `modules.<name>.enable = true`.
- Each module defines `publicApi.version = 3` and a set of operations.
- Each operation can expose:
  - app: `svc::<service>::<operation>`
  - hook env var: `SVC_<SERVICE>_<OP>`
- Service apps require `PROJECT_ENV` and use `NIX_ENV` for slot selection (`0` default when unset).
- Log and event operations are available as:
  - `svc::<service>::log`
  - `svc::<service>::events`

## Module pages

- `docs/modules/postgres.md`
- `docs/modules/nginx.md`
- `docs/modules/minio.md`
- `docs/modules/reth.md`
- `docs/modules/helios.md`
