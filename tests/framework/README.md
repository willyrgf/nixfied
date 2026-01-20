# Framework Tests

This folder contains fixtures and guidance for the Nixfied framework tests.

## Run

From the framework repo root:

```bash
nix run .#framework::test
```

If you have untracked changes and `nix run .#framework::test` fails to see them,
use a path-based flake reference:

```bash
nix run path:.#framework::test
```

## What is covered

The test runner validates:
- Flake evaluation (`nix flake show`, `nix flake check --no-build`).
- Core apps (`help`, `dev`, `test`, `build`, `check`, `ci`).
- CI DSL behavior (modes, step skipping, cleanup, artifacts).
- Module hooks for Postgres and Nginx (init + config generation).
- Supervisor config generation.
- Installer safety and filter behavior.

## Fixtures

- `fixtures/ci/ci.nix`
  - CI DSL test configuration.
- `fixtures/modules/conf.nix`
  - Base config with Postgres/Nginx enabled.
- `fixtures/modules/dev.nix`
  - Dev command script to exercise module hooks and helpers.
