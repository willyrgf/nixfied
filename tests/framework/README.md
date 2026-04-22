# Framework Tests

This directory documents the framework repository test surface.

## Primary Command

Run from repository root:

```bash
nix run .#test -- --mode full --summary
```

The public source-repo entrypoint is `test`.
It resolves to the native `workflow.test.<mode>` family through `nixfied/framework/testing/repo-overlay.nix`.

The authoritative selection catalog lives in `nixfied/framework/testing/catalog.nix`.
`tests/framework/framework-test-catalog.nix` is a thin projection that adds the concrete flake check imports, and `tests/framework/default.nix` is the final flake-check projection.
This README is an overview, not the canonical full check list.

## Profiles

Available profiles:

- `feature-proof`: run only direct feature proofs
- `ci`: run canonical PR coverage
- `full`: run every registered framework check plus the self-host step

## Shards

Available shards:

- `compile`
- `manifest`
- `kernel`
- `adapters`
- `e2e`
- `migration`

The shards are an internal execution layout for the source-repo framework workflows.
They follow the current ownership layers, and `services` is intentionally gone.
Shard targeting is internal now and exposed through `run-task` rather than a separate public app.

## Useful Commands

```bash
nix run .#test -- --mode feature-proof --summary
nix run .#test -- --mode ci --summary
nix run .#test -- --mode full --summary
nix run .#run-task -- task.test.framework.feature-proof.compile
nix run .#run-task -- task.test.framework.ci.kernel
nix run .#run-task -- task.test.framework.full.e2e
```

## Layer Intent

- `compile`: compile-time model, help, schema, documentation, and packaging proofs
- `manifest`: manifest-owned feature proofs and manifest handoff invariants between Nix and kernel
- `kernel`: kernel-owned workflow, validation, registry, summary, and run semantics
- `adapters`: thin shell and launcher process-edge behavior only
- `e2e`: user-facing public behavior, install and upgrade flows, isolation, and self-host execution
- `migration`: low-scope legacy-regression and ownership-migration checks

## Canonical Sources

Use `tests/framework/default.nix` as the source of truth for the complete registered check list and exact check names.

Use `nixfied/framework/testing/catalog.nix` as the source of truth for:

- shard order
- shard membership
- `feature-proof`, `ci`, and `full` profile selection

## Reuse Guides

Use these checked-in guides when reviewing framework behavior or teaching an agent how the current framework surface works:

- `tests/framework/WORKFLOW_REUSE.md`
- `tests/framework/SERVICE_LIFECYCLE_API.md`

## Output Contract

All framework-facing command output must remain ASCII and prefix-based:

- `INFO:`
- `WARN:`
- `ERROR:`
- `OK:`
- `SKIP:`
