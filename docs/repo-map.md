# Repository Map

Generated from `docs/repo-index.json`.

## Start Here
- `docs/repo-index.json` - Canonical deterministic repository index.
- `docs/repo-map.md` - LLM-facing repository map generated from docs/repo-index.json.
- `README.md` - Primary repository overview and command entrypoints.
- `docs/DETAILED.md` - Detailed model architecture and contracts.
- `docs/UPGRADE.md` - Downstream upgrade notes for behavioral and path contract changes.
- `docs/ARCHITECTURE.md` - High-level architecture reference.
- `AGENTS.md` - Agent instructions and collaboration constraints.

## Components
- `flake.nix` (nix-flake)
- `nixfied/framework/runtime/kernel/Cargo.toml` (rust-cargo)

## Command Surfaces
- `build` from `nixfied/project/tasks.nix`
- `check` from `nixfied/project/tasks.nix`
- `check-ports` from `nixfied/modules/operations.nix`
- `ci` from `nixfied/project/tasks.nix`
- `dev` from `nixfied/project/tasks.nix`
- `docs` from `nixfied/framework/core/mkCoreSurfaces.nix`
- `features` from `nixfied/framework/core/mkCoreSurfaces.nix`
- `format` from `nixfied/project/tasks.nix`
- `framework::install` from `nixfied/framework/presets/install.nix`
- `framework::upgrade` from `nixfied/framework/presets/install.nix`
- `health` from `nixfied/modules/operations.nix`
- `introspect` from `nixfied/framework/core/mkCoreSurfaces.nix`
- `ports` from `nixfied/modules/operations.nix`
- `ready` from `nixfied/modules/operations.nix`
- `schema` from `nixfied/framework/core/mkNixfied.nix`
- `stateHash` from `nixfied/framework/core/mkNixfied.nix`
- `test` from `nixfied/framework/testing/repo-overlay.nix`
- `test-isolation` from `nixfied/modules/operations.nix`
- `validate-env` from `nixfied/modules/operations.nix`

## Features
- `runtime.ephemeral.env-file-loading` [runtime] - Ephemeral host env file loading policy (coverage required)
- `runtime.ephemeral.include-untracked` [runtime] - Ephemeral worktree copy policy (coverage required)
- `runtime.ephemeral.source-materialization` [runtime] - Ephemeral source materialization defaults (coverage required)
- `runtime.output.prefix-contract` [runtime] - Stable ASCII log prefix contract (coverage required)
- `runtime.registry.isolation` [runtime] - Run-scoped registry isolation (coverage required)
- `runtime.service-operations` [runtime] - Compiler-published service operation ABI and runtime service dispatch (coverage required)
- `runtime.workflow-service-phases` [runtime] - Workflow preRun/postRun service phases over compiled service-set policy (coverage required)
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
- `task.framework.upgrade` [task] - Upgrade vendored wrapper in-place (coverage required)
- `task.ops.check-ports` [task] - Check model-derived port availability (coverage required)
- `task.ops.health` [task] - Run service health checks (coverage required)
- `task.ops.ports` [task] - Print model-derived port assignments (coverage required)
- `task.ops.ready` [task] - Run service readiness checks (coverage required)
- `task.ops.test-isolation` [task] - Run isolation checks (coverage required)
- `task.ops.validate-env` [task] - Validate slot/env settings (coverage required)
- `task.test` [task] - Run tests (coverage required)
- `task.test.framework.ci.adapters` [task] - Framework adapters checks (ci)
- `task.test.framework.ci.compile` [task] - Framework compile checks (ci)
- `task.test.framework.ci.kernel` [task] - Framework kernel checks (ci)
- `task.test.framework.ci.manifest` [task] - Framework manifest checks (ci)
- `task.test.framework.ci.migration` [task] - Framework migration checks (ci)
- `task.test.framework.feature-proof.compile` [task] - Framework compile checks (feature-proof)
- `task.test.framework.feature-proof.e2e` [task] - Framework e2e checks (feature-proof)
- `task.test.framework.feature-proof.manifest` [task] - Framework manifest checks (feature-proof)
- `task.test.framework.full.adapters` [task] - Framework adapters checks (full)
- `task.test.framework.full.compile` [task] - Framework compile checks (full)
- `task.test.framework.full.e2e` [task] - Framework e2e checks (full)
- `task.test.framework.full.kernel` [task] - Framework kernel checks (full)
- `task.test.framework.full.manifest` [task] - Framework manifest checks (full)
- `task.test.framework.full.migration` [task] - Framework migration checks (full)
- `task.test.framework.selfhost` [task] - Framework self-host smoke command
- `task.test.isolation.probe` [task] - Isolation probe
- `task.test.isolation.unit` [task] - Isolation probe unit
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
- `workflow.test.ci` [workflow] - Framework test workflow (ci) (coverage required)
- `workflow.test.feature-proof` [workflow] - Framework test workflow (feature-proof) (coverage required)
- `workflow.test.framework.selfhost` [workflow] - Framework self-host smoke workflow (coverage required)
- `workflow.test.full` [workflow] - Framework test workflow (full) (coverage required)
- `workflow.test.isolation.probe` [workflow] - Isolation probe workflow (coverage required)
- `workflow.test.parallel.failfast` [workflow] - Parallel runner fail-fast workflow (coverage required)
- `workflow.test.parallel.smoke` [workflow] - Parallel runner smoke workflow (coverage required)

## Runtime and Introspection
- `run-task -- <task-id> [--exclude-services <csv>] [-- ...]` from `nixfied/framework/runtime/engine.nix`
- `run-workflow -- <workflow-id> [--exclude-services <csv>] [-- ...]` from `nixfied/framework/runtime/engine.nix`
- `run-workflow-parallel -- <workflow-id> [--exclude-services <csv>] [-- ...]` from `nixfied/framework/runtime/engine.nix`
- `runs [run-id]` from `nixfied/framework/runtime/engine.nix`
- `stop-run -- <run-id>` from `nixfied/framework/runtime/engine.nix`
- `stop-all-runs` from `nixfied/framework/runtime/engine.nix`
- `docs`, `features`, `help`, `introspect`, `stateHash`, `schema` from `nixfied/framework/core/mkCoreSurfaces.nix`

## Sensitive Zones
- `nixfied/framework` - Framework-owned runtime, install internals, and source-repo test overlay; avoid direct edits in installed repos. (checks: nix run .#help)
- `nixfied/project/conf.nix` - Project identity, environment names, and port contract. (checks: nix run .#validate-env,nix run .#ci -- --summary)
- `nixfied/project/module.nix` - Primary command/task/workflow surface. (checks: nix run .#help,nix run .#test -- --mode full --summary,nix run .#ci -- --summary)

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
- `nix run .#framework::install`
- `nix run .#framework::upgrade`

## Invariants
- Treat `nixfied/project/` as the primary customization surface.
- Keep command metadata aligned with script behavior.
- Keep this map and `docs/repo-index.json` in sync when command surfaces or key docs change.
