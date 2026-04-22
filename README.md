# Nixfied (Model-First)

Nixfied is a model-driven Nix framework.

The canonical source of truth is `nixfiedModel`, compiled from typed modules and exposed through stable flake app surfaces.

## Quick Start

```bash
nix run .#help
nix run .#dev
nix run .#test
nix run .#ci -- --summary
```

## Core Commands

Model-generated app surfaces:

- `nix run .#build`
- `nix run .#check`
- `nix run .#check-ports`
- `nix run .#ci`
- `nix run .#dev`
- `nix run .#format`
- `nix run .#framework::install`
- `nix run .#framework::upgrade`
- `nix run .#health`
- `nix run .#ports`
- `nix run .#ready`
- `nix run .#test`
- `nix run .#test-isolation`
- `nix run .#validate-env`

Runtime surfaces:

- `nix run .#run-task -- <task-id> [--exclude-services <csv>] [-- ...]`
- `nix run .#run-workflow -- <workflow-id> [--exclude-services <csv>] [-- ...]`
- `nix run .#run-workflow-parallel -- <workflow-id> [--exclude-services <csv>] [-- ...]`
- `nix run .#runs [-- <run-id>]`
- `nix run .#stop-run -- <run-id>`
- `nix run .#stop-all-runs`

Introspection surfaces:

- `nix run .#docs`
- `nix run .#features`
- `nix run .#introspect -- <query>`
- `nix run .#stateHash`
- `nix run .#schema`

## Configuration Surface

Primary edit points:

- `nixfied/project/conf.nix` for project identity, envs, ports, and module settings.
- `nixfied/project/module.nix` for project-layer composition, plus `nixfied/project/{tasks,workflows}.nix` for project-owned command surfaces.
- `nixfied/modules/` for typed module options.

Environment defaults:

- `NIX_ENV=0`
- `PROJECT_ENV=dev`

Service configuration and selectors:

- Configure services in `nixfied/project/conf.nix` under `services.<name>` (`enable`, `ports`, `sources`, `defaultSource`).
- Service probes accept composable selectors on health/readiness checks: `--service <name|all>` and optional `--source <key>`.
- `nix run .#health -- --service <name|all> [--source <key>]`
- `nix run .#ready -- --service <name|all> [--source <key>]`
- See `docs/modules/README.md` for service-specific configuration details.

Service exclusion controls:

- Invocation-time exclusion is `--exclude-services <csv>`.
- Static repo-owned graph exclusion is `nixfied.graph.excludedServices = [ "<service>" ]`.
- Runtime exclusion suppresses service operations for that invocation only.
- Graph exclusion happens before project service projection and is the correct mechanism when a service branch must not be evaluated at all.

Examples:

```bash
# Exclude api during one workflow run.
nix run .#run-workflow -- workflow.ci.full --exclude-services api

# Exclude cache during one task run.
nix run .#run-task -- <task-id> --exclude-services cache

# Exclude multiple services for one run.
nix run .#ci -- --exclude-services helios,reth --summary
```

If a workflow unit depends on an excluded service unit, its dependents are cascade-skipped with reason `dependency-skipped`. An exclusion-only workflow exits 0.

Task and workflow service requirements:

- Use `requirements.services = [ "<service>" ... ]` on tasks and workflow units to declare hard service capability requirements.
- Compile-time graph exclusion and runtime `--exclude-services` both use those requirements.

Ephemeral execution defaults (configured in `nixfied/project/conf.nix`):

- `ephemeral.copyMode = "git-files"`: copies tracked + non-ignored untracked files.
- `ephemeral.excludePatterns = [...]`: fallback excludes for `static-excludes` copy mode.
- `ephemeral.keepFailures = true`: failed ephemeral roots are retained for debugging.
- `ephemeral.maxFailedRoots = 8`: cap on retained failed roots.
- `ephemeral.maxFailedRootAgeHours = 72`: age-based pruning for retained failed roots.
- `ephemeral.maxCopyBytes = 0`: disabled by default; set `> 0` to enforce a pre-copy size cap.
- `ephemeral.minFreeBytesAfterCopy = 0`: disabled by default; set `> 0` to enforce free-space floor after copy estimate.

Ephemeral runtime behavior:

- Success path: ephemeral root is cleaned.
- Failure path: root is preserved (or dropped if `keepFailures = false`), then pruned by age/count policy.
- Budget checks run before copy and fail fast with `ERROR:` if limits are exceeded.

## Architecture Summary

- Modules: `lib.evalModules` + typed options from `nixfied/modules/*.nix`.
- Compiler: deterministic pass pipeline in `nixfied/compiler/*.nix`.
- Hashing: `stateHash = sha256(toCanonicalNix(model))`.
- Runtime engine: public flake apps exec the `nixfied-runtime` entrypoint, which owns invocation-time selection, run inventory, stop controls, summaries, and service dispatch.
- Registry: append-only NDJSON event stream with replay support.
- Runtime semantics: `nixfied-kernel` is the framework-owned validation and runtime-semantics layer for scheduling, run-record/registry/summary state, runtime-event policy/status projection, and probe execution; shell launchers stage env/path/process orchestration and invoke kernel commands.
- Runtime inputs: the kernel consumes compiled runtime assets only; it does not evaluate modules, run compiler passes, or regenerate launchers.
- Compiler-owned published artifacts include the execution graph, service surface catalog, validation IR, contract bundle/docs, and introspection graph/bundle.
- Ownership: framework-owned code lives under `nixfied/framework/{core,runtime,install,presets}`; `nixfied/project/` is the downstream composition/customization layer.

## Runtime Policy

- Zero framework CUE.
- Zero framework `jq` in framework runtime/build-check paths.
- No validator shim or shell-owned semantic sidecars/engines for run records, summaries, registry/runtime status, or service-policy/probe behavior.
- The surviving public service ABI is `svc::<service>::<op>`.
- The surviving public invocation-time service selector is `--exclude-services <csv>`.

## Docs

- `docs/ARCHITECTURE.md` for high-level architecture.
- `docs/DETAILED.md` for model, runtime, workflow, and registry contracts.
- `docs/UPGRADE.md` for downstream upgrade notes and behavior changes.
- `docs/repo-map.md` for the generated repository map.
- `docs/modules/README.md` for module-specific configuration notes.
- `tests/framework/README.md` for deterministic framework validation.

## API

Primary flake-output entrypoint:

```nix
nixfied.lib.mkFlakeOutputs {
  system = "x86_64-linux";
  projectRoot = ./.;
  projectModules = [ ./nixfied/project/module.nix ];
  extraModules = [ ];
  localOverrides = [ ];
}
```

Lower-level compiled-output entrypoint:

```nix
nixfied.lib.mkNixfied {
  system = "x86_64-linux";
  projectRoot = ./.;
  projectModules = [ ./nixfied/project/module.nix ];
  extraModules = [ ];
  localOverrides = [ ];
}
```

`localOverrides` is explicit. The default repository flake passes `[]`, so `nixfied/local/default.nix` is preserved template space, not an auto-loaded module.

`mkFlakeOutputs` publishes thin runtime-backed apps. The surviving invocation-time selector is `--exclude-services <csv>`, for example:

```bash
nix run .#ci -- --exclude-services helios --mode full --summary
nix run .#run-task -- task.test.framework.feature-proof.e2e --exclude-services helios
```

Compiled outputs also export the surviving public service operation surfaces for enabled services:

```bash
nix run .#svc::<service>::<op>
```

Project tasks should prefer `svc <service> <op>` or `svc::...` apps over importing `nixfied/framework/runtime/services/...` directly. That keeps excluded services out of the compiled closure.

Returned attributes:

```nix
{
  model;
  stateHash;
  tasks;
  workflows;
  apps;
  packages;
  checks;
  devShells;
}
```

## Output Contract

User-facing shell task output should be plain ASCII and prefix-based:

- `INFO:`
- `WARN:`
- `ERROR:`
- `OK:`
- `SKIP:`

## Parallel Worker Reuse Example

Use the parallel runner for multiple workflow runs, and cap worker reuse with `NIXFIED_CI_MAX_WORKERS` (or `CI_MAX_WORKERS`):

```bash
# Run a workflow with its modeled maxWorkers limit.
nix run .#run-workflow-parallel -- workflow.test.parallel.smoke --summary

# Reuse the same parallel runner for another workflow, capped to 2 workers.
NIXFIED_CI_MAX_WORKERS=2 nix run .#run-workflow-parallel -- workflow.ci.full --summary
```

## CI Parallel Integration

`nix run .#ci` uses the CI workflow model, with `execution.parallel = true` in `workflow.ci.*`.
You can cap concurrency with `NIXFIED_CI_MAX_WORKERS` (or `CI_MAX_WORKERS`):

```bash
NIXFIED_CI_MAX_WORKERS=2 nix run .#ci -- --mode full --summary
```

For serial debugging, override the workflow setting:

```bash
NIXFIED_WORKFLOW_PARALLEL=0 nix run .#ci -- --mode full --summary
```

In the source repo, `nix run .#test` is the public framework validation surface and accepts `--mode feature-proof|ci|full`.
GitHub Actions runs `nix run .#test -- --mode ci --summary` on pull requests and `nix run .#test -- --mode full --summary` on branch builds.
