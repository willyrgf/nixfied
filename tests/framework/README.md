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
- Helper functions (log_capture, summary_parse, wait_http/port, start_service, with_service, with_cleanup).
- Slot/env helpers (SLOT_INFO ports, REQUIRE_SLOT_ENV prompt behavior).
- CI DSL behavior (modes, errors, step skipping, cleanup, teardown, artifacts, summary output).
- Module hooks for Postgres and Nginx (init, start/stop, config generation).
- Supervisor config generation.
- Installer safety, re-entry reuse, invalid filter handling, prompt-plan toggle, and framework marker/app exposure behavior.

## Fixtures

- `fixtures/ci/ci.nix`
  - CI DSL test configuration.
- `fixtures/ci/retention.nix`
  - CI artifacts retention behavior.
- `fixtures/modules/conf.nix`
  - Base config with Postgres/Nginx enabled.
- `fixtures/modules/dev.nix`
  - Dev command script to exercise module hooks and helpers.
- `fixtures/helpers/runtime.sh`
  - Helper function regression tests (runtime helpers).
- `fixtures/slots/runtime.sh`
  - Slot/env helper regression tests.
