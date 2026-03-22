# Repository Map

Generated from `docs/repo-index.json`.

## Start Here
- `docs/repo-index.json` - Canonical deterministic repository index.
- `docs/repo-map.md` - LLM-facing repository map generated from docs/repo-index.json.
- `README.md` - Primary repository overview and command entrypoints.
- `CLEANUPS.md` - Current repository cleanup ledger and maintenance queue.
- `docs/DETAILED.md` - Detailed model architecture and contracts.
- `docs/UPGRADE.md` - Downstream upgrade notes for behavioral and path contract changes.
- `docs/ARCHITECTURE.md` - High-level architecture reference.
- `AGENTS.md` - Agent instructions and collaboration constraints.

## Components
- `flake.nix` (nix-flake)

## Command Surfaces
- `build` from `nixfied/project/tasks.nix`
- `check` from `nixfied/project/tasks.nix`
- `check-ports` from `nixfied/modules/operations.nix`
- `ci` from `nixfied/project/tasks.nix`
- `dev` from `nixfied/project/tasks.nix`
- `docs` from `nixfied/framework/runtime/dispatcher.nix`
- `features` from `nixfied/framework/runtime/dispatcher.nix`
- `format` from `nixfied/project/tasks.nix`
- `framework::install` from `nixfied/framework/presets/install.nix`
- `framework::test` from `nixfied/framework/presets/framework-test.nix`
- `framework::upgrade` from `nixfied/framework/presets/install.nix`
- `health` from `nixfied/modules/operations.nix`
- `model` from `nixfied/framework/core/mkNixfied.nix`
- `ports` from `nixfied/modules/operations.nix`
- `ready` from `nixfied/modules/operations.nix`
- `schema` from `nixfied/framework/core/mkNixfied.nix`
- `services` from `nixfied/framework/core/mkNixfied.nix`
- `stateHash` from `nixfied/framework/core/mkNixfied.nix`
- `tasks` from `nixfied/framework/core/mkNixfied.nix`
- `test` from `nixfied/project/tasks.nix`
- `test-isolation` from `nixfied/modules/operations.nix`
- `validate-env` from `nixfied/modules/operations.nix`

## Features
- `runtime.ephemeral.env-file-loading` [runtime] - Ephemeral host env file loading policy (coverage required)
- `runtime.ephemeral.include-untracked` [runtime] - Ephemeral worktree copy policy (coverage required)
- `runtime.ephemeral.source-materialization` [runtime] - Ephemeral source materialization defaults (coverage required)
- `runtime.output.prefix-contract` [runtime] - Stable ASCII log prefix contract (coverage required)
- `runtime.registry.isolation` [runtime] - Run-scoped registry isolation (coverage required)
- `service.helios` [service] - Service configuration and runtime surface for helios (coverage required)
- `service.minio` [service] - Service configuration and runtime surface for minio (coverage required)
- `service.nginx` [service] - Service configuration and runtime surface for nginx (coverage required)
- `service.postgres` [service] - Service configuration and runtime surface for postgres (coverage required)
- `service.reth` [service] - Service configuration and runtime surface for reth (coverage required)
- `task.build` [task] - Build artifacts (coverage required)
- `task.check` [task] - Run quality checks (coverage required)
- `task.ci` [task] - Run the CI pipeline (coverage required)
- `task.ci.nginx-proxy` [task] - Nginx proxy test
- `task.ci.quality` [task] - Quality checks
- `task.ci.system-quick` [task] - Quick system tests
- `task.ci.tests` [task] - Tests
- `task.dev` [task] - Start the dev workflow (coverage required)
- `task.format` [task] - Format Nix files (coverage required)
- `task.framework.install` [task] - Install thin or vendored wrapper flake (coverage required)
- `task.framework.test` [task] - Run framework validation in the model (coverage required)
- `task.framework.upgrade` [task] - Upgrade vendored wrapper in-place (coverage required)
- `task.ops.check-ports` [task] - Check model-derived port availability (coverage required)
- `task.ops.health` [task] - Run service health checks (coverage required)
- `task.ops.ports` [task] - Print model-derived port assignments (coverage required)
- `task.ops.ready` [task] - Run service readiness checks (coverage required)
- `task.ops.test-isolation` [task] - Run isolation checks (coverage required)
- `task.ops.validate-env` [task] - Validate slot/env settings (coverage required)
- `task.test` [task] - Run tests (coverage required)
- `task.test.framework.selfhost` [task] - Framework self-host smoke command
- `task.test.parallel.fail` [task] - Parallel fail-fast trigger
- `task.test.parallel.skip` [task] - Parallel smoke when-skip unit
- `task.test.parallel.sleep-a` [task] - Parallel smoke unit A
- `task.test.parallel.sleep-b` [task] - Parallel smoke unit B
- `task.test.parallel.sleep-c` [task] - Parallel smoke dependent unit
- `task.test.parallel.sleep-d` [task] - Parallel smoke lock unit
- `task.test.parallel.slow-a` [task] - Parallel fail-fast slow unit A
- `task.test.parallel.slow-b` [task] - Parallel fail-fast slow unit B
- `workflow.ci.app` [workflow] - App CI workflow (coverage required)
- `workflow.ci.basic` [workflow] - Basic CI workflow (coverage required)
- `workflow.ci.env` [workflow] - Environment CI workflow (coverage required)
- `workflow.ci.full` [workflow] - Full CI workflow (coverage required)
- `workflow.test.framework.selfhost` [workflow] - Framework self-host smoke workflow (coverage required)
- `workflow.test.parallel.failfast` [workflow] - Parallel runner fail-fast workflow (coverage required)
- `workflow.test.parallel.smoke` [workflow] - Parallel runner smoke workflow (coverage required)

## Dispatcher and Introspection
- `run-task -- <task-id> [-- ...]` from `nixfied/framework/runtime/dispatcher.nix`
- `run-workflow -- <workflow-id> [-- ...]` from `nixfied/framework/runtime/dispatcher.nix`
- `run-workflow-parallel -- <workflow-id> [-- ...]` from `nixfied/framework/runtime/dispatcher.nix`
- `runs [run-id]` from `nixfied/framework/runtime/dispatcher.nix`
- `stop-run -- <run-id>` from `nixfied/framework/runtime/dispatcher.nix`
- `stop-all-runs` from `nixfied/framework/runtime/dispatcher.nix`
- `features` from `nixfied/framework/runtime/dispatcher.nix`
- `introspect`, `stateHash`, `schema` from `nixfied/framework/core/mkNixfied.nix`

## Sensitive Zones
- `nixfied/framework` - Framework-owned presets, runtime, and install internals; avoid direct edits in installed repos. (checks: nix run .#help)
- `nixfied/project/conf.nix` - Project identity, environment names, and port contract. (checks: nix run .#validate-env, nix run .#ci -- --summary)
- `nixfied/project/module.nix` - Project-layer composition and shared builders. (checks: nix run .#help, nix run .#framework::test, nix run .#ci -- --summary)

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
- `nix run .#features`
- `nix run .#framework::test`
- `nix run .#framework::install`
- `nix run .#framework::upgrade`

## Invariants
- Treat `nixfied/project/` as the primary customization surface.
- Keep command metadata aligned with script behavior.
- Keep this map and `docs/repo-index.json` in sync when command surfaces or key docs change.
