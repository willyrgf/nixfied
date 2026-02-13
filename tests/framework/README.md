# Framework Tests

This folder contains fixtures and guidance for the Nixfied framework tests.

## Run

From the framework repo root:

```bash
nix run .#framework::test
```

Default profile is `ci`.
Default shard worker count is `2`.
Use `FRAMEWORK_ISOLATION=1` to include the isolation runner.

Useful options:

```bash
nix run .#framework::test -- --jobs 3
nix run .#framework::test -- --serial
nix run .#framework::test -- --list-shards
nix run .#framework::test -- --shard installer
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
- Helper functions (log_capture, summary_parse, wait_http/port, start_service, start_service_into, with_service, with_cleanup).
- Fixture helper behavior (`fixture_start_service`) including readiness precedence (`READY` over `HEALTH`) and `keep_running`.
- Slot/env helpers (SLOT_INFO explicit env/slot validation, REQUIRE_SLOT_ENV failure paths).
- CI DSL behavior (modes, errors, step skipping, cleanup, teardown, artifacts, summary output, summary.json).
- Service module hooks for Postgres, Nginx, MinIO, Reth, and Helios (init/start/stop/check-config plus health/ready pass-fail paths).
- Supervisor config generation/hook exposure, readiness enforcement, and service-level health checks.
- Ephemeral slot locking (acquire/release, env var export).
- Run registry (foreground run tracking, meta.json, output.log).
- Module apps exposure (db-*, nginx-*, supervisor apps present/absent based on config).
- Installer safety, upgrade (preserving nixfied/project), re-entry reuse, invalid filter handling, prompt-plan toggle, and framework marker/app exposure behavior.

## Example snippets

These are intentionally minimal, real patterns taken from fixtures:

```bash
# Start a temporary service, run a check, then auto-cleanup.
with_service web --wait-port "$PORT" -- python3 -m http.server "$PORT" --bind 127.0.0.1 --run \
  "$BASH" -c "nc -z 127.0.0.1 $PORT"

# Generate a CI artifact path (creates directory if needed).
ART_PATH=$(artifact_path "quality.log")
log_capture "$ART_PATH" -- ./lint

# Resolve slot/env values explicitly.
export PROJECT_ENV="test"
export NIX_ENV="0"
eval "$(${SLOT_INFO})"
echo "$ENV"   # test
echo "$SLOT"  # 0
```

## Examples index

If you want end‑to‑end examples, start here:
- `fixtures/helpers/runtime.nix` — helper utilities in real scripts (log_capture, with_service, artifact_path).
- `fixtures/slots/runtime.nix` — slot/env strict validation and alias behavior.
- `fixtures/ci/ci.nix` — CI DSL wiring and step control.
- `fixtures/ci/retention.nix` — artifact retention modes.
- `fixtures/ci/unknown-step.nix` — failure on misconfigured steps.
- `fixtures/modules/dev.nix` — service hook lifecycle with health/readiness behavior across all supported modules.
- `fixtures/ephemeral/lock.nix` — Ephemeral slot lock acquire/release.
- `fixtures/registry/foreground.nix` — Run registry foreground tracking.

## Fixtures

- `fixtures/ci/ci.nix`
  - CI DSL test configuration.
- `fixtures/ci/retention.nix`
  - CI artifacts retention behavior.
- `fixtures/ci/unknown-step.nix`
  - CI modes referencing unknown steps.
- `fixtures/ci/summary-json.nix`
  - CI summary.json output validation.
- `fixtures/modules/conf.nix`
  - Base config with Postgres/Nginx enabled.
- `fixtures/modules/dev.nix`
  - Dev command script to exercise module hooks and helpers.
- `fixtures/helpers/runtime.nix`
  - Helper function regression tests (runtime helpers).
- `fixtures/slots/runtime.nix`
  - Slot/env helper regression tests.
- `fixtures/ephemeral/lock.nix`
  - Ephemeral slot locking (acquire/release) tests.
- `fixtures/registry/foreground.nix`
  - Run registry foreground mode tests (meta.json, output.log).
