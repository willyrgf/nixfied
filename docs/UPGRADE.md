# Upgrade Notes

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
