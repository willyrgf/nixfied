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

Compatibility shims remain in place for downstream imports:

- `nixfied/lib/`
- `nixfied/install/`
- `nixfied/runner/`
- `nixfied/registry/`

Vendored metadata is slightly richer now:

- `nixfied/VENDORED.txt` records the framework source revision
- upgrade writes an exact `old..new` commit summary when the framework source is a git worktree
- packaged-source installs fall back to a concise note (plus a compare hint when both revisions are known)
- that packaged-source fallback is the normal path for vendored `framework::upgrade`, because the wrapper dispatches through `github:willyrgf/nixfied/...` instead of your local `.git` checkout

## Race-Free Isolation Guarantees

This upgrade changes runtime state defaults and strengthens write/lock guarantees.

The goal is to remove shared mutable state across workspaces and make run metadata safe to consume while a run is in progress.

## User-Visible Changes

- Default registry state is now workspace-scoped: `/tmp/nixfied-runtime/<projectId>/<workspaceId>/registry`.
- Default artifacts are now workspace-scoped: `/tmp/ci-artifacts/<projectId>/<workspaceId>`.
- Ephemeral workflows now propagate isolated `HOME`, `TMPDIR`, `XDG_DATA_HOME`, `XDG_STATE_HOME`, `XDG_CACHE_HOME`, `REGISTRY_ROOT`, `CI_ARTIFACTS_DIR`, and `NIXFIED_SERVICE_ROOT`.
- Ephemeral runs preserve relative caller subdirectories inside the copied source tree instead of collapsing back to project root.
- If `REGISTRY_ROOT` is explicitly overridden and artifacts still use the legacy default, artifacts now follow the registry under `$REGISTRY_ROOT/artifacts`.
- Orchestrator run records, workflow `summary.json`, registry sequence updates, and registry event appends now use atomic write/replace behavior.
- Registry locks now persist owner metadata to make timeout and stale-lock diagnosis explicit.

## Downstream Impact

Downstream code may need adjustment if it depended on the previous shared defaults.

Common examples:

- Scripts hardcoding `/tmp/ci-artifacts` or shared registry paths.
- Tasks assuming ephemeral runs still use host-global `HOME`, `TMPDIR`, or XDG cache/state directories.
- Tooling reading run JSON or `summary.json` during writes and relying on partial in-place updates.
- Overrides that set `REGISTRY_ROOT` but implicitly expected artifacts to remain in the old shared default location.

## Upgrade Checklist

- Replace hardcoded registry/artifact paths with `REGISTRY_ROOT`, `CI_ARTIFACTS_DIR`, or model-derived state.
- Treat ephemeral `HOME`, `TMPDIR`, and `XDG_*` values as run-scoped, not machine-global.
- If you launch from a subdirectory, validate that relative workdir behavior inside ephemeral mode is what your tasks expect.
- If you override `REGISTRY_ROOT`, verify whether artifact co-location under `$REGISTRY_ROOT/artifacts` is desired.
- Keep using explicit `CI_ARTIFACTS_ROOT` or `CI_ARTIFACTS_DIR` when you need a fixed artifact location.

## Recommended Validation

- `nix run .#framework::test`
- `nix flake check path:. -L`
- `nix run .#run-workflow -- <workflow-id> --summary`
- Re-run one workflow with explicit `REGISTRY_ROOT` and one with explicit `CI_ARTIFACTS_DIR` to confirm downstream expectations.
