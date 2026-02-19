# Nixfied Detailed Architecture and Full Usage Guide

This document is the maintainers' canonical deep spec for Nixfied architecture, contracts, enforcement behavior, and full user workflows.

It complements:
- `README.md` (entrypoint and quick reference)
- `docs/modules/*.md` (module operation catalogs)

It does not change runtime behavior. It documents current behavior and enforcement points in code.

## 1) Purpose and Audience

Primary audience:
- Framework maintainers and contributors who need to understand "why" and "how" the framework is built and enforced.

Secondary audience:
- Project operators and users who need complete end-to-end usage playbooks.

This document is normative for:
- Architecture and evaluation flow
- Public API contracts and enforcement points
- Upgrade boundaries and extension rules

This document is not the operation inventory for each module. For full operation lists, see:
- `docs/modules/postgres.md`
- `docs/modules/nginx.md`
- `docs/modules/minio.md`
- `docs/modules/reth.md`
- `docs/modules/helios.md`

## 2) Design Goals and Non-Goals

### Design Goals

1. One configuration surface:
- Project behavior is defined in `nixfied/project/` and composed into one flake app surface.

2. Contract-first command interfaces:
- Every app exposes structured API metadata (`api.version = 2`, `appContract.version = 2`) and fails fast if invalid.

3. Contract-first service modules:
- Every enabled module must expose `publicApi.version = 3` with validated operations and runtime primitive schema.

4. Strict runtime primitives:
- `LOG_LEVEL` and `OUTPUT_MODE` are normalized, validated, and aliased consistently across apps and services.

5. Slot/env isolation:
- Slot/env selection is explicit and validated before service/supervisor operations run.

6. Deterministic generated command surfaces:
- Service apps, hook env vars, supervisor apps, process apps, and isolation apps are generated from validated specs.

7. Safe upgrade boundaries:
- Framework-owned files are overwritten on upgrade; user-owned surfaces are preserved by default.

### Non-Goals

1. This document is not a beginner-only quickstart.
2. This document does not duplicate every module operation description from `docs/modules/*`.
3. This document does not promise backward compatibility for all commits (project is development-stage).

## 3) Architecture Overview

Nixfied composes project config, framework internals, and optional modules into a validated `apps` output.

High-level flow:

```text
nixfied/project/*.nix
  -> merged project config
  -> slots/env helpers
  -> optional service modules (postgres/nginx/minio/reth/helios)
  -> service contract validation (publicApi v3)
  -> hook env generation
  -> framework lib assembly
  -> app generation (core + module + supervisor + process + utility + ci + isolation + framework + local)
  -> app contract validation (api/appContract v2)
  -> flake outputs.apps / outputs.packages / outputs.devShells
```

### Ownership Boundaries

Framework-owned:
- `flake.nix`
- `flake.lock`
- `nixfied/.framework/`

User-owned customization surfaces:
- `nixfied/project/`
- `nixfied/local/` (optional)

Canonical boundary record:
- `nixfied/VENDORED.txt`

## 4) Composition and Evaluation Pipeline

The pipeline is assembled in `flake.nix`.

### Pipeline Steps

1. Load project config:
- `project = import ./nixfied/project { ... };`

2. Build slot/env helpers:
- `nixfied/.framework/slots.nix`

3. Conditionally load enabled modules:
- `nixfied/.framework/postgres`
- `nixfied/.framework/nginx`
- `nixfied/.framework/minio`
- `nixfied/.framework/reth`
- `nixfied/.framework/helios`

4. Collect and validate service APIs:
- `serviceApis = { <enabled service>.publicApi ... }`
- `validateEnabledServicesHaveContracts`

5. Optionally load ephemeral subsystem:
- `nixfied/.framework/ephemeral.nix` when `project.ephemeral.enable = true`

6. Build hook environment:
- `nixfied/.framework/hooks.nix`
- Exports slot helpers, service hooks, supervisor hooks, ephemeral hooks.

7. Build framework lib:
- `nixfied/.framework/lib/default.nix`

8. Generate module/supervisor/process/utility apps:
- `nixfied/.framework/internal/module-apps.nix`

9. Build core apps:
- `nixfied/.framework/internal/core.nix`

10. Build CI app:
- `nixfied/.framework/ci.nix`

11. Build isolation apps:
- `nixfied/.framework/internal/isolation.nix`

12. Build framework-only helper apps (workspace only):
- `framework::<name>` wrappers over internal install/test apps when `nixfied/.framework/.workspace` exists.

13. Optionally import local extensions:
- `nixfied/local` merged into apps/packages/devShells.

14. Validate all apps against API contract:
- `appsValidated = lib.appApi.validateApps apps0`

15. Expose final outputs:
- `apps`
- `packages`
- `devShells`

SOURCE:
- `flake.nix`
- `nixfied/.framework/internal/core.nix`
- `nixfied/.framework/internal/module-apps.nix`
- `nixfied/.framework/internal/isolation.nix`
- `nixfied/.framework/lib/app-api.nix`

## 5) Runtime Model: Env, Slot, Ports, and Directories

### Core Runtime Variables

- `PROJECT_ENV` (or project-specific env var from config): logical environment (`dev`, `test`, `prod`, custom).
- `NIX_ENV` (or configured slot var): slot number.
- `NIXFIED_ENV`: compatibility alias for slot value.

Behavior:
- `PROJECT_ENV` is required for slot/env-required service/supervisor contexts.
- `NIX_ENV` defaults to configured `slots.default` when unset.
- Invalid slot or env fails fast.

SOURCE:
- `nixfied/.framework/slots.nix`

### Effective Port Formula

For each configured port key:

```text
effective_port = base_port + (slot * slots.stride) + env_offset
```

Where:
- `base_port` comes from `project.ports.<name>`
- `slot` comes from slot var (`NIX_ENV` by default)
- `env_offset` comes from `project.envs.<env>.offset`

### Directory Model

Per slot/env directories are derived from `directories.base`, for example:
- `LOG_DIR`
- `RUN_DIR`
- `CONFIG_DIR`
- `STATE_DIR`
- `BACKUP_BASE_DIR`

Per-service directories and socket directories are also derived and exported.

### Slot/Env Runtime Accessors

Shell helpers and JSON providers:
- `resolveEnv`
- `resolveSlot`
- `requireSlotEnv`
- `requireSlotEnvJson`
- `getSlotInfo`
- `getSlotInfoJson`

Hook exports include:
- `SLOT_INFO`
- `SLOT_INFO_JSON`
- `REQUIRE_SLOT_ENV`
- `REQUIRE_SLOT_ENV_JSON`

SOURCE:
- `nixfied/.framework/slots.nix`
- `nixfied/.framework/hooks.nix`
- `nixfied/.framework/lib/slot-env-runtime.nix`

## 6) Public API Surfaces

This section defines the framework's public interfaces for users and automation.

### 6.1 Command Namespace API

Core:
- `help`
- project commands from `nixfied/project/*.nix` (`dev`, `test`, `build`, `check`, `format`, usually `ci`)

Service:
- `svc::<service>::<operation>`

Supervisor:
- `up`
- `down`
- `svc-status`
- `svc-health`
- `svc-logs`
- `svc-restart`

Process registry:
- `process::status`
- `process::slots`
- `process::runs`
- `process::inspect`
- `process::stop`
- `process::gc`

Utility:
- `ports`
- `check-ports`

Isolation:
- `validate-env`
- `test-isolation`

Framework workspace-only:
- `framework::install`
- `framework::upgrade`
- `framework::prompt-plan`
- `framework::test`

Local extensions:
- `nixfied/local` can add `apps`, `packages`, `devShells`.

Notes:
- `help` is generated from project commands + module apps; isolation apps are runnable even when not listed in help output.

SOURCE:
- `flake.nix`
- `nixfied/.framework/internal/core.nix`
- `nixfied/.framework/internal/module-apps.nix`
- `nixfied/.framework/internal/isolation.nix`
- `nixfied/.framework/internal/install-manifest.nix`

### 6.2 Project Configuration API

Primary user customization surface:
- `nixfied/project/conf.nix`
- `nixfied/project/dev.nix`
- `nixfied/project/test.nix`
- `nixfied/project/prod.nix`
- `nixfied/project/quality.nix`
- `nixfied/project/format.nix`
- `nixfied/project/ci.nix`

Merged by:
- `nixfied/project/default.nix`

Command declarations are merged under:
- `commands.<name>`

SOURCE:
- `nixfied/project/default.nix`
- `nixfied/project/conf.nix`

### 6.3 App API Contract (`api.version = 2`)

Every app in flake outputs must provide valid `meta.nixfied.api`.

Required top-level API fields:
- `version = 2`
- `summary`
- `details`
- `usage`
- `appContract`

`appContract` requirements:
- `version = 2`
- `name`
- `commandClass` in `typed|passthrough|json|batch-runner`
- `allowUnknownArgs` policy must match command class
- `args` spec list
- `env` spec list
- `outputs.mode` in `text|kv|json`
- `failureCodes` positive exit code map
- `idempotent` boolean

Command class policy:
- `typed` requires `allowUnknownArgs=false`, cannot use `outputs.mode=json`.
- `passthrough` requires `allowUnknownArgs=true`.
- `json` requires `outputs.mode=json` and `allowUnknownArgs=false`.
- `batch-runner` requires `allowUnknownArgs=false`.

Runtime primitive env specs are required in app contracts:
- `LOG_LEVEL` + alias `NIXFIED_LOG_LEVEL`
- `OUTPUT_MODE` + alias `NIXFIED_OUTPUT_MODE`

SOURCE:
- `nixfied/.framework/lib/app-api.nix`
- `nixfied/.framework/lib/shell-contract.nix`

### 6.4 Service API Contract (`publicApi.version = 3`)

Every enabled service module must expose valid `publicApi`.

Required fields:
- `version = 3`
- `service` (must match service key)
- `summary`
- `details`
- `operations` attrset
- `artifacts` attrset
- `runtimePrimitives`

Required lifecycle operations:
- `start`
- `stop`
- `status`

Operation schema supports:
- `script` (string/path/derivation)
- `summary`
- `details`
- optional `usage`, `examples`, `args`, `env`, `category`, `class`, `idempotent`
- optional `hook`
- optional `exposeApp`
- optional `appName`

Generated outputs:
- App (when `exposeApp` is not `false`): `svc::<service>::<operation>`
- Hook env var: `SVC_<SERVICE>_<HOOK_OR_OP>`

SOURCE:
- `nixfied/.framework/lib/service-api.nix`

### 6.5 Runtime Primitive API

Canonical variables:
- `LOG_LEVEL`: `error|warn|info|debug|trace`
- `OUTPUT_MODE`: `stdout|logs|both`

Aliases:
- `NIXFIED_LOG_LEVEL`
- `NIXFIED_OUTPUT_MODE`

Defaults:
- Derived from `project.logging.level` and `project.logging.output`.
- If `OUTPUT_MODE` is unset, `LOG_LEVEL=debug`, and output default is `stdout`, resolved mode becomes `both`.

Strict enforcement:
- Conflicting canonical/alias values fail.
- Empty explicit values fail (must unset variable to use defaults).
- Invalid enum values fail.

SOURCE:
- `nixfied/.framework/lib/shell-contract.nix`
- `nixfied/.framework/lib/service-api.nix`
- `nixfied/project/conf.nix`

### 6.6 Service Reuse/Ownership/Discovery Policy API

Policy env vars:
- `SERVICE_REUSE_POLICY`: `never|same-root|same-slot|cross-run`
- `SERVICE_OWNER_SCOPE`: `ephemeral|persistent`
- `SERVICE_DISCOVERY_SCOPE`: `local|global`

Matrix constraints:
- `cross-run` requires `owner=persistent` and `discovery=global`.
- `same-root` requires `owner=ephemeral` and `discovery=local`.

Inference behavior in registry runtime:
- Owner/discovery defaults depend on ephemeral vs persistent execution context.
- Reuse policy can be inferred from owner/discovery.

SOURCE:
- `nixfied/.framework/lib/service-policy.nix`
- `nixfied/.framework/lib/process-registry.nix`

### 6.7 Hook API

Global hook env exported for app scripts:
- Slot/env helpers (`SLOT_INFO`, `SLOT_INFO_JSON`, `REQUIRE_SLOT_ENV`, `REQUIRE_SLOT_ENV_JSON`)
- Service operation launchers (`SVC_*`)
- Supervisor hooks (`SUPERVISOR_*`) when supervisor enabled
- Ephemeral hooks (`EPHEMERAL_*`) when ephemeral enabled

SOURCE:
- `nixfied/.framework/hooks.nix`

## 7) Contract Enforcement Internals

This section documents where contracts are enforced and what failures look like.

### 7.1 Evaluation-Time Enforcement

ENFORCES:
- App metadata correctness before app exposure.
- Service contract correctness before service hook/app generation.
- Enabled service coverage (no missing public contracts).

Key functions:
- `validateApiErrors`, `validateApi`, `validateApps` in `nixfied/.framework/lib/app-api.nix`
- `validateAppContractErrors` in `nixfied/.framework/lib/shell-contract.nix`
- `validateServiceApiErrors`, `validateServiceApis`, `validateEnabledServicesHaveContracts` in `nixfied/.framework/lib/service-api.nix`

FAILS_IF:
- Missing or malformed `api` metadata.
- Invalid command class/outputs/allowUnknown policy combinations.
- Missing runtime primitive env specs in app contract.
- Service `publicApi` missing/invalid/version mismatch.
- Missing required lifecycle operations.
- Invalid runtime primitive schema for services.

### 7.2 Generation-Time Enforcement

ENFORCES:
- Service hook env var generation has no name collisions.
- `exposeApp = false` operations are hook-only.

Key functions:
- `hookNameFor`
- `mkServiceHookEnvFromContract`
- `mkServiceAppsFromContract`

FAILS_IF:
- Duplicate hook names across operations/services.

SOURCE:
- `nixfied/.framework/lib/service-api.nix`

### 7.3 Runtime Enforcement in App Launchers

ENFORCES:
- Runtime primitive normalization and alias consistency.
- Runtime env/arg/exit validation against `appContract`.
- Slot/env requirements for service and supervisor operation launchers.

Where:
- Generic app launcher: `mkAppScript` in `nixfied/.framework/lib/builders.nix`
- Service op launcher: `mkServiceOpLauncher` in `nixfied/.framework/lib/service-api.nix`
- Supervisor app wrappers: `mkSupervisorHookApp` in `nixfied/.framework/internal/module-apps.nix`

FAILS_IF:
- `PROJECT_ENV` missing in slot/env-required contexts.
- Slot/env values invalid.
- Runtime primitive alias conflicts, empty explicit values, or invalid enums.
- App args/env/exit code violate contract.

SOURCE:
- `nixfied/.framework/lib/builders.nix`
- `nixfied/.framework/lib/shell-contract.nix`
- `nixfied/.framework/lib/service-api.nix`
- `nixfied/.framework/internal/module-apps.nix`
- `nixfied/.framework/lib/slot-env-runtime.nix`

## 8) Module System Architecture

### 8.1 Shared Module Contract

All service modules must provide a validated `publicApi` contract:
- `publicApi.version = 3`
- `publicApi.runtimePrimitives.version = 1`
- operations with lifecycle minimum (`start`, `stop`, `status`)

Operation materialization:
- app: `svc::<service>::<operation>` (unless `exposeApp = false`)
- hook env: `SVC_<SERVICE>_<OP_OR_HOOK>`

Observability operations:
- `log`
- `events`

SOURCE:
- `nixfied/.framework/lib/service-api.nix`
- `nixfied/.framework/lib/service-observability.nix`

### 8.2 Postgres Module Architecture

Entry:
- `nixfied/.framework/postgres/default.nix`

Pattern:
- Custom aggregator builds explicit `publicApi` using `serviceApi.mkServiceApiV3`.
- Composes lifecycle, backup, migration, migration-safety, rollback, and port-management scripts.

Notable architecture points:
- Rich operation surface for lifecycle, migrations, backups, port management, observability.
- Hook-only operation:
  - `ensure-migration-tested` with `exposeApp = false`
- Artifacts include port metadata, PGDATA expression, default/test database names.

Reference operation docs:
- `docs/modules/postgres.md`

### 8.3 Nginx Module Architecture

Entry:
- `nixfied/.framework/nginx/default.nix`

Pattern:
- Custom aggregator builds explicit `publicApi` using `serviceApi.mkServiceApiV3`.
- Composes lifecycle, site management, TLS operations, and observability.

Notable architecture points:
- Site/TLS operation families are part of public service contract.
- Hook-only operations:
  - `site-proxy` (`exposeApp = false`)
  - `site-static` (`exposeApp = false`)
- Artifacts export HTTP/HTTPS port var names and data directory expression.

Reference operation docs:
- `docs/modules/nginx.md`

### 8.4 MinIO Module Architecture

Entry:
- `nixfied/.framework/minio/default.nix`

Pattern:
- Uses shared module factory `serviceModule.mkServiceModule`.
- Operations defined once, with log/events appended by factory via observability helpers.

Notable architecture points:
- Lifecycle + bucket/admin operation families.
- No hook-only operations by default (all declared operations exposed as apps/hooks).
- Artifacts include API/console port vars and data directory.

Reference operation docs:
- `docs/modules/minio.md`

### 8.5 Reth Module Architecture

Entry:
- `nixfied/.framework/reth/default.nix`

Pattern:
- Uses `serviceModule.mkServiceModule`.
- Lifecycle-oriented operation set with readiness and full-start variants.

Notable architecture points:
- Artifacts include HTTP/WS/auth port vars, data directory, network, dev mode.
- Operations are app-exposed and hook-exported.

Reference operation docs:
- `docs/modules/reth.md`

### 8.6 Helios Module Architecture

Entry:
- `nixfied/.framework/helios/default.nix`

Pattern:
- Uses `serviceModule.mkServiceModule`.
- Lifecycle-oriented operation set with readiness and full-start variants.

Notable architecture points:
- Readiness tunables are documented in operation details.
- Artifacts include RPC/execution port vars, data directory, network.
- Operations are app-exposed and hook-exported.

Reference operation docs:
- `docs/modules/helios.md`

### 8.7 Shared Module Factory (`service-module`)

Factory:
- `nixfied/.framework/lib/service-module.nix`

Responsibilities:
- Build `publicApi` via `mkServiceApiV3`.
- Add `log` and `events` operations automatically.
- Return exported scripts plus `publicApi`.

## 9) Supervisor, Process Registry, and Isolation

### 9.1 Supervisor App Generation

Generated from fixed specs in:
- `nixfied/.framework/internal/module-apps.nix`

Apps:
- `up`, `down`, `svc-status`, `svc-health`, `svc-logs`, `svc-restart`

Execution model:
- Apps load slot/env context via `requireSlotEnvJson`.
- Apps invoke `run_hook SUPERVISOR_*`.
- Some apps forward args (`svc-logs`, `svc-restart`).

### 9.2 Process Registry Architecture

Primary implementation:
- `nixfied/.framework/lib/process-registry.nix`

Key properties:
- Global event ledger and snapshot under `process.registryRoot`.
- CLI tools exposed as `process::*` apps.
- Tracks runs, services, slot occupancy, and lifecycle state.
- Enforces service reuse/ownership/discovery policy matrix.

### 9.3 Isolation Apps

Implementation:
- `nixfied/.framework/internal/isolation.nix`

`validate-env`:
- Requires slot info JSON context.
- Validates configured ports, required directories, and service sockets.
- Emits `ERROR:` and `WARN:` lines; exits non-zero on errors.

`test-isolation`:
- Runs matrix over configured slots/envs.
- Starts run command per matrix cell (default run is CI summary mode).
- Re-runs validation on interval until completion/timeout.
- Produces pass/fail summary and per-run logs.

## 10) CI and Ephemeral Execution Architecture

### 10.1 CI DSL and Runner

CI generator:
- `nixfied/.framework/ci.nix`

High-level model:
- Validate CI schema from project config.
- Build mode plans from `ci.modes` and `ci.steps`.
- Execute plan units (via execution core), collect results and summary.

Features:
- `--mode <name>` or `--<mode>`
- `--summary` output mode for concise reporting
- `--bg` delegates to run registry for detached execution
- `summary.json` artifact per run

SOURCE:
- `nixfied/.framework/ci.nix`
- `nixfied/.framework/lib/execution-core.nix`
- `nixfied/.framework/lib/run-registry.nix`

### 10.2 Ephemeral Execution

Implementation:
- `nixfied/.framework/ephemeral.nix`

Model:
- Acquire slot lock unless caller already sets slot.
- Create isolated ephemeral root.
- Copy source tree with exclusions.
- Set `XDG_DATA_HOME` into ephemeral root.
- Run command with optional dependency install.
- Keep ephemeral state on failure; cleanup on success.

When active:
- CI can use ephemeral wrapper (`ci.useEphemeral`).
- Process registry events capture slot acquisition/release and ownership context.

## 11) Install/Upgrade and Vendoring Boundaries

Install/upgrade implementation:
- `nixfied/.framework/internal/install.nix`
- `nixfied/.framework/internal/install-manifest.nix`

### Commands

- `framework::install`
- `framework::upgrade`
- `framework::prompt-plan`

### Boundary Behavior

By default upgrade preserves:
- `nixfied/project/`
- `nixfied/local/`

Overwritten framework-managed files:
- `flake.nix`, `flake.lock`
- `nixfied/.framework/`

`--reset-project`:
- Overwrites project template files in `nixfied/project/`.

Canonical boundary record:
- `nixfied/VENDORED.txt`

## 12) Failure Modes and Enforcement Matrix

| Condition | ENFORCES (source) | Failure signal | Typical remediation |
|---|---|---|---|
| Missing app metadata (`api`) | `nixfied/.framework/lib/app-api.nix` | Flake eval error: app API contract violated | Add `commands.<name>.api` or use `mkNixfiedApp` |
| `api.version != 2` or missing summary/details/usage | `nixfied/.framework/lib/app-api.nix` | Flake eval error with field-level diagnostics | Fix API fields to v2 schema |
| Invalid `appContract` class/policy mismatch | `nixfied/.framework/lib/shell-contract.nix` | Flake eval error in appContract validation | Align `commandClass`, `allowUnknownArgs`, `outputs.mode` |
| Missing runtime primitive env specs in app contract | `nixfied/.framework/lib/shell-contract.nix` | Flake eval error requiring `LOG_LEVEL` and `OUTPUT_MODE` env specs | Use `mkCommandApi`/default contract helpers |
| Missing service `publicApi` for enabled service | `nixfied/.framework/lib/service-api.nix` | Flake eval error from `validateEnabledServicesHaveContracts` | Ensure module exposes `publicApi` |
| `publicApi.version != 3` or malformed service contract | `nixfied/.framework/lib/service-api.nix` | Flake eval error with service-specific diagnostics | Fix contract shape and required fields |
| Service missing required lifecycle ops | `nixfied/.framework/lib/service-api.nix` | Flake eval error listing missing ops | Add `start`, `stop`, `status` operations |
| Invalid runtime primitive schema for service contract | `nixfied/.framework/lib/service-api.nix` | Flake eval error on `runtimePrimitives` | Use `mkRuntimePrimitivesV1` or exact schema |
| Hook name collision across service operations | `nixfied/.framework/lib/service-api.nix` | Flake eval error: hook name collision | Rename op hook token or operation names |
| Missing `PROJECT_ENV` in service/supervisor context | `nixfied/.framework/lib/slot-env-runtime.nix` and `nixfied/.framework/slots.nix` | Runtime error (`ERROR:`) + non-zero exit | Export valid env (`PROJECT_ENV=dev` etc.) |
| Invalid slot value | `nixfied/.framework/slots.nix` | Runtime error for slot range/type | Set integer slot within configured bounds |
| `LOG_LEVEL`/`OUTPUT_MODE` alias conflicts or empty explicit values | `nixfied/.framework/lib/shell-contract.nix` | Runtime error (`ERROR:`) + exit 2 | Set one canonical var or matching values; unset empty vars |
| Invalid service policy matrix | `nixfied/.framework/lib/service-policy.nix` | Runtime error (`ERROR:`) | Use allowed combinations for reuse/owner/discovery |
| Missing slot info runtime in isolation validation | `nixfied/.framework/internal/isolation.nix` | Runtime error (SLOT_INFO_JSON required) | Run via app context (`nix run .#validate-env`) |

## 13) Extension Playbook for Contributors

### 13.1 Add or Modify a Project Command

1. Edit one of:
- `nixfied/project/dev.nix`
- `nixfied/project/test.nix`
- `nixfied/project/prod.nix`
- `nixfied/project/quality.nix`
- `nixfied/project/format.nix`
- `nixfied/project/ci.nix`

2. Define/modify `commands.<name>` with `api` contract.
3. Prefer `commandLib.mkCommand` in `nixfied/project/lib/command.nix`.
4. Keep `usage`, `summary`, `details`, args/env docs in sync with script behavior.

### 13.2 Add or Modify a Service Operation

1. Edit service module `operations` in module `default.nix`.
2. Provide required operation fields:
- `script`, `summary`, `details`
3. Optional:
- `hook` for stable hook naming
- `usage` examples
- `exposeApp = false` for hook-only operations
4. Keep module docs (`docs/modules/<service>.md`) in sync.

### 13.3 Add a New Service Module

1. Create module directory under `nixfied/.framework/<module>/`.
2. Implement config + lifecycle scripts.
3. Expose `publicApi` via:
- direct `serviceApi.mkServiceApiV3`, or
- `serviceModule.mkServiceModule`.
4. Wire module loading and service API collection in `flake.nix`.
5. Add module docs under `docs/modules/`.

### 13.4 Add Local Extensions

1. Create `nixfied/local/default.nix` (or import structure).
2. Expose any of:
- `apps`
- `packages`
- `devShells`
3. Keep app metadata valid (`meta.nixfied.api`).

## 14) User Point of View: Full Framework Usage

This section is task-oriented. Each playbook includes goal, command sequence, expected outcome, and common failure modes.

### Playbook 1: Bootstrap and First Validation

Goal:
- Confirm framework installation, inspect command surface, and validate baseline config.

Commands:

```bash
nix run .#help
nix run .#check
nix run .#ci -- --summary
```

Expected result:
- Command list renders.
- Quality checks pass.
- CI summary runs with structured result output.

Common failures:
- Missing command metadata: fix `commands.<name>.api` under `nixfied/project/`.
- Discovery drift in docs map/index: run check with refresh mode if needed.

### Playbook 2: Daily Developer Loop

Goal:
- Use Nixfied as the primary dev/test/build/check interface.

Commands:

```bash
nix run .#dev
nix run .#test
nix run .#build
nix run .#check
nix run .#format
```

Expected result:
- Each command executes with app contract validation and consistent logging primitives.

Common failures:
- Contract failures for args/env/exit behavior: align script behavior with `appContract`.
- Incorrect log routing: set `OUTPUT_MODE=stdout|logs|both`.

### Playbook 3: Service Lifecycle Workflows

Goal:
- Manage module services by slot/env safely and deterministically.

Precondition:
- Module enabled in `nixfied/project/conf.nix` (`modules.<name>.enable = true`).

Commands:

```bash
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::postgres::full-start
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::postgres::ready
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::postgres::status
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::postgres::log -- --lines 200
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::postgres::events -- --limit 50
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::postgres::stop
```

Expected result:
- Service commands run in slot/env-aware context with consistent runtime primitive handling.

Common failures:
- `PROJECT_ENV` missing: set it explicitly.
- Slot/env mismatch: verify `NIX_ENV` and configured env name.
- Port conflict: inspect with `nix run .#check-ports`.

### Playbook 4: Supervisor and Multi-Service Operations

Goal:
- Control all supervisor-managed services as a group.

Commands:

```bash
PROJECT_ENV=dev NIX_ENV=0 nix run .#up
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc-status
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc-health
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc-logs -- --follow
PROJECT_ENV=dev NIX_ENV=0 nix run .#down
```

Expected result:
- Supervisor hooks orchestrate services through generated apps.

Common failures:
- Missing supervisor hook env: ensure module/supervisor wiring remains intact.
- Slot/env not exported: run commands with explicit env vars.

### Playbook 5: Module-Specific Operations (Examples)

Postgres migrations and shell:

```bash
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::postgres::test-migrations
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::postgres::shell -- -c "select 1;"
```

MinIO buckets:

```bash
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::minio::full-start
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::minio::bucket-ensure -- artifacts
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::minio::bucket-list
```

Nginx site management:

```bash
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::nginx::init
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::nginx::site-add -- example.localhost 127.0.0.1 3000
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::nginx::reload
```

Expected result:
- Domain-specific operations execute through the same validated service launcher path.

For full operation lists and examples:
- `docs/modules/README.md`
- `docs/modules/*.md`

### Playbook 6: CI and Background Execution

Goal:
- Run CI in foreground or background and inspect tracked state.

Commands:

```bash
nix run .#ci -- --summary
nix run .#ci -- --mode app
nix run .#ci -- --bg
nix run .#process::runs
nix run .#process::status -- --all
```

Expected result:
- Background runs are registered and inspectable.
- CI artifacts include `summary.json`.

Common failures:
- Unknown mode: verify `ci.modes` in `nixfied/project/ci.nix`.
- Stale or stuck runs: use `process::inspect` and `process::stop`.

### Playbook 7: Isolation and Parallel Safety Validation

Goal:
- Validate isolation guarantees across slot/env matrix.

Commands:

```bash
nix run .#validate-env
nix run .#test-isolation
nix run .#test-isolation -- --keep-logs
```

Expected result:
- Port/listener/directory/socket checks run per matrix entry.
- Final summary indicates validation errors/warnings and run failures.

Common failures:
- Missing runtime context when not invoked through app wrapper.
- Port or directory issues per slot/env; inspect preserved logs.

### Playbook 8: Debugging and Recovery

Goal:
- Diagnose command/service failures with structured runtime controls.

Commands:

```bash
LOG_LEVEL=debug OUTPUT_MODE=both nix run .#check
PROJECT_ENV=dev NIX_ENV=0 LOG_LEVEL=debug OUTPUT_MODE=both nix run .#svc::postgres::status
nix run .#ports
nix run .#check-ports
nix run .#process::inspect -- <run-id-or-service>
```

Expected result:
- Increased observability and deterministic failure diagnostics.

Common failures and fixes:
- Conflicting aliases:
  - Do not set conflicting `LOG_LEVEL` and `NIXFIED_LOG_LEVEL`.
  - Do not set conflicting `OUTPUT_MODE` and `NIXFIED_OUTPUT_MODE`.
- Empty runtime variables:
  - Unset variable instead of setting empty string.
- Invalid policy env matrix:
  - Align `SERVICE_REUSE_POLICY`, `SERVICE_OWNER_SCOPE`, and `SERVICE_DISCOVERY_SCOPE`.

### Playbook 9: Upgrade Workflow

Goal:
- Upgrade framework safely while preserving project-owned customization.

Commands:

```bash
nix run .#framework::upgrade -- --force
nix run .#help
nix run .#check
nix run .#ci -- --summary
```

Expected result:
- Framework files updated.
- `nixfied/project/` and `nixfied/local/` preserved by default.

Common failures:
- Dirty branch/worktree constraints: use `--worktree` or clean working tree.
- Need template reset: pass `--reset-project` intentionally.

## 15) Compatibility and Breaking-Change Policy

Current policy:
- Nixfied is in active development.
- Breaking changes can happen between commits.

Compatibility anchors:
- App API version (`api.version = 2`)
- App contract version (`appContract.version = 2`)
- Service API version (`publicApi.version = 3`)
- Service runtime primitive schema version (`runtimePrimitives.version = 1`)

When changing contracts:
1. Update code validators and generators.
2. Update `README.md` and this document.
3. Update module docs as needed.
4. Run framework and project checks before merge.

## 16) Verification Checklist for Documentation and Contract Changes

When changing architecture/contracts/docs, run:

```bash
nix run .#help
nix run .#check
nix run .#ci -- --summary
```

For framework changes:

```bash
nix run .#framework::test
```

Doc consistency checklist:
1. `README.md` command/API sections still align with code.
2. `docs/modules/*.md` operation summaries still align with module contracts.
3. This document still points to current source-of-truth files.
4. Any new command namespace appears in documentation.
5. Any new contract field has explicit enforcement source documented.

Recommended pre-merge grep checks:

```bash
rg "api.version|appContract.version|publicApi.version|runtimePrimitives.version" nixfied/.framework nixfied/project
rg "svc::|process::|framework::|validate-env|test-isolation" README.md docs
```

---

## Appendix A: Source-of-Truth Map by Concern

Architecture composition:
- `flake.nix`

App contract validation and app metadata:
- `nixfied/.framework/lib/app-api.nix`
- `nixfied/.framework/lib/shell-contract.nix`
- `nixfied/.framework/lib/builders.nix`

Service contract validation and app/hook generation:
- `nixfied/.framework/lib/service-api.nix`
- `nixfied/.framework/lib/service-module.nix`
- `nixfied/.framework/lib/service-observability.nix`

Slot/env runtime and port/directory derivation:
- `nixfied/.framework/slots.nix`
- `nixfied/.framework/lib/slot-env-runtime.nix`
- `nixfied/.framework/hooks.nix`

Generated module/supervisor/process/utility apps:
- `nixfied/.framework/internal/module-apps.nix`

CI and isolation:
- `nixfied/.framework/ci.nix`
- `nixfied/.framework/internal/isolation.nix`
- `nixfied/.framework/lib/execution-core.nix`
- `nixfied/.framework/lib/run-registry.nix`

Ephemeral execution:
- `nixfied/.framework/ephemeral.nix`

Process registry and policy:
- `nixfied/.framework/lib/process-registry.nix`
- `nixfied/.framework/lib/service-policy.nix`

Install/upgrade boundaries:
- `nixfied/.framework/internal/install.nix`
- `nixfied/.framework/internal/install-manifest.nix`
- `nixfied/VENDORED.txt`

Project customization API:
- `nixfied/project/conf.nix`
- `nixfied/project/default.nix`
- `nixfied/project/lib/command.nix`
