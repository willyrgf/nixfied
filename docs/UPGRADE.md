# Upgrade Notes

## Framework Reorganization

Framework-owned code now has an explicit home:

- `nixfied/framework/core/`: compiler-facing core helpers and `mkNixfied`
- `nixfied/framework/runtime/`: dispatcher, executor, registry, runtime helpers, and service runtimes
- `nixfied/framework/install/`: wrapper/install internals
- `nixfied/framework/presets/`: framework-owned install/test/self-host presets

Project-owned customization is now separated from framework-owned behavior:

- `nixfied/project/module.nix` is the composition layer
- `nixfied/project/{runtime,services,tasks,workflows}.nix` hold project-owned definitions
- `nixfied/local/` remains optional extension space and is still preserved on vendored upgrade

All supported framework imports now use the canonical `nixfied/framework/...` paths directly.

Generated vendored wrappers now import canonical framework-core entrypoints directly:

- `./nixfied/framework/core/default.nix`
- `./nixfied/framework/core/framework-revision.nix`

Vendored metadata is slightly richer now:

- `nixfied/VENDORED.txt` records the framework source revision
- upgrade writes an exact `old..new` commit summary when the framework source is a git worktree
- packaged-source installs fall back to a concise note (plus a compare hint when both revisions are known)
- that packaged-source fallback is the normal path for vendored `framework::upgrade`, because the wrapper dispatches through `github:willyrgf/nixfied/...` instead of your local `.git` checkout

## Race-Free Isolation Guarantees

This upgrade changes runtime state defaults and strengthens write/lock guarantees.

The goal is to remove shared mutable state across workspaces and make run metadata safe to consume while a run is in progress.

## Model and Runtime Separation

The framework now separates the cheap canonical model from heavy service runtime materialization.

Main changes:

- Exported `model.services` has been removed from the canonical model. Use `model.serviceCatalog` instead.
- Heavy normalized service runtime remains internal as `compiled.services` and is materialized only for selected execution surfaces.
- `stateHash` now represents the cheap canonical model.
- Runtime execution identity uses a separate deterministic `runtimeHash`.
- Public task/workflow launchers and dispatcher surfaces are selector-aware two-stage entrypoints.
- `svc::<service>::<op>` surfaces now materialize only the named service runtime.

This split is a real contract change for downstream code that previously read deep runtime service config from the exported model.

## User-Visible Changes

- Default registry state is now workspace-scoped: `${NIX_BUILD_TOP:-${XDG_CACHE_HOME:-$HOME/.cache}}/nixfied-runtime/<projectId>/<workspaceId>/registry`.
- Default artifacts are now workspace-scoped: `/tmp/nixfied-artifacts-<projectId>-<workspaceId>`.
- Ephemeral workflows now propagate isolated `HOME`, `TMPDIR`, `XDG_DATA_HOME`, `XDG_STATE_HOME`, `XDG_CACHE_HOME`, `REGISTRY_ROOT`, `CI_ARTIFACTS_DIR`, and `NIXFIED_SERVICE_ROOT`.
- Ephemeral runs preserve relative caller subdirectories inside the copied source tree instead of collapsing back to project root.
- If `REGISTRY_ROOT` is explicitly overridden and no explicit artifacts root is provided, artifacts follow the registry under `$REGISTRY_ROOT/artifacts`.
- Orchestrator run records, workflow `summary.json`, registry sequence updates, and registry event appends now use atomic write/replace behavior.
- Registry locks now persist owner metadata to make timeout and stale-lock diagnosis explicit.
- Tasks without selected services no longer receive ambient per-service `SVC_*` hook env vars.
- Tasks without selected services no longer receive ambient per-service `NIXFIED_SERVICE_*` env vars.
- If a task needs service hooks or per-service env, declare that need through `requirements.services` or wrap the service app explicitly.

## Downstream Impact

Downstream code may need adjustment if it depended on the previous shared defaults.

Common examples:

- Downstream callers importing removed shim paths; update them to canonical `nixfied/framework/...` imports.
- Downstream tooling reading `model.services`; migrate to `model.serviceCatalog` for cheap introspection and to internal compiled runtime surfaces only when heavy service details are actually required.
- Tasks or scripts that assumed all `SVC_*` or `NIXFIED_SERVICE_*` vars were ambient; model service needs explicitly instead.
- Scripts hardcoding `/tmp/ci-artifacts`, `/tmp/nixfied-artifacts-*`, or shared registry paths.
- Tasks assuming ephemeral runs still use host-global `HOME`, `TMPDIR`, or XDG cache/state directories.
- Tooling reading run JSON or `summary.json` during writes and relying on partial in-place updates.
- Overrides that set `REGISTRY_ROOT` without also setting `CI_ARTIFACTS_ROOT` or `CI_ARTIFACTS_DIR`.

## Upgrade Checklist

- Replace hardcoded registry/artifact paths with `REGISTRY_ROOT`, `CI_ARTIFACTS_DIR`, or model-derived state.
- Update any framework imports to the canonical `nixfied/framework/...` paths.
- Replace reads of `model.services` with `model.serviceCatalog`.
- Audit tasks that rely on ambient `SVC_*` or `NIXFIED_SERVICE_*` vars and move those dependencies into `requirements.services`.
- Treat ephemeral `HOME`, `TMPDIR`, and `XDG_*` values as run-scoped, not machine-global.
- If you launch from a subdirectory, validate that relative workdir behavior inside ephemeral mode is what your tasks expect.
- If you override `REGISTRY_ROOT` without explicit artifact settings, verify whether artifact co-location under `$REGISTRY_ROOT/artifacts` is desired.
- Keep using explicit `CI_ARTIFACTS_ROOT` or `CI_ARTIFACTS_DIR` when you need a fixed artifact location.

## Recommended Validation

- `nix run .#framework::test`
- `nix flake check . -L`
- `nix run .#run-workflow -- <workflow-id> --summary`
- Re-run one workflow with explicit `REGISTRY_ROOT` and one with explicit `CI_ARTIFACTS_DIR` to confirm downstream expectations.
