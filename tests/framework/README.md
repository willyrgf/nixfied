# Framework Tests

This directory documents the native framework test surface.

## Primary Command

Run from repository root:

```bash
nix run .#framework::test
```

`framework::test` is a first-class framework preset task defined in `nixfied/framework/presets/framework-test.nix`.
It is organized around the current ownership model instead of the deleted shell-heavy control layers.

The authoritative check registry and shard catalog live in `tests/framework/framework-test-catalog.nix`.
`tests/framework/default.nix` is a thin projection over that catalog.
This README is an overview, not the canonical full check list.

## Profiles

Available profiles:

- `feature-proof`: run only direct feature proofs
- `ci`: run canonical feature proofs plus the `compile`, `manifest`, `kernel`, `adapters`, and `migration` shards
- `full`: run every registered framework check

## Shards

Available shards:

- `compile`
- `manifest`
- `kernel`
- `adapters`
- `e2e`
- `migration`

The shards are an execution layout for `framework::test`.
They follow the current ownership layers, and `services` is intentionally gone.

## Useful Commands

```bash
nix run .#framework::test -- --list-shards
nix run .#framework::test -- --profile feature-proof --summary
nix run .#framework::test -- --profile full --summary
nix run .#framework::test -- --shard compile --summary
nix run .#framework::test -- --shard migration --summary
nix run .#framework::test -- --profile ci --summary-json /tmp/framework-test-summary.json
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

Use `tests/framework/framework-test-catalog.nix` as the source of truth for:

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
