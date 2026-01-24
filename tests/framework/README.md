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
- Module hooks for Postgres and Nginx (init, start/stop, config generation, error paths).
- Supervisor config generation.
- Installer safety, re-entry reuse, invalid filter handling, prompt-plan toggle, and framework marker/app exposure behavior.

## Example snippets

These are intentionally minimal, real patterns taken from fixtures:

```bash
# Start a temporary service, run a check, then auto-cleanup.
with_service web --wait-port "$PORT" -- python3 -m http.server "$PORT" --bind 127.0.0.1 --run \
  "$BASH" -c "nc -z 127.0.0.1 $PORT"

# Generate a CI artifact path (creates directory if needed).
ART_PATH=$(artifact_path "quality.log")
log_capture "$ART_PATH" -- ./lint

# Resolve env defaults from the command name.
export COMMAND_NAME="ci"
eval "$(${SLOT_INFO})"
echo "$ENV"  # test
```

## Examples index

If you want end‑to‑end examples, start here:
- `fixtures/helpers/runtime.nix` — helper utilities in real scripts (log_capture, with_service, artifact_path).
- `fixtures/slots/runtime.nix` — slot/env resolution patterns and prompt behavior.
- `fixtures/ci/ci.nix` — CI DSL wiring and step control.
- `fixtures/ci/retention.nix` — artifact retention modes.
- `fixtures/ci/unknown-step.nix` — failure on misconfigured steps.
- `fixtures/modules/dev.nix` — Postgres/Nginx hook usage + error paths.

## Fixtures

- `fixtures/ci/ci.nix`
  - CI DSL test configuration.
- `fixtures/ci/retention.nix`
  - CI artifacts retention behavior.
- `fixtures/ci/unknown-step.nix`
  - CI modes referencing unknown steps.
- `fixtures/modules/conf.nix`
  - Base config with Postgres/Nginx enabled.
- `fixtures/modules/dev.nix`
  - Dev command script to exercise module hooks and helpers.
- `fixtures/helpers/runtime.nix`
  - Helper function regression tests (runtime helpers).
- `fixtures/slots/runtime.nix`
  - Slot/env helper regression tests.
