# Framework Tests

This directory documents the native framework test surface.

## Primary Command

Run from repository root:

```bash
nix run .#framework::test
```

`framework::test` is a first-class framework preset task defined in `nixfied/framework/presets/framework-test.nix`.
It is organized around the post-refactor ownership model instead of the deleted shell-heavy control layers.

The authoritative check registry lives in `tests/framework/default.nix`.
The authoritative shard catalog lives in `tests/framework/framework-test-shards.nix`.
This README is an overview, not the canonical full check list.

## Profiles

Available profiles:

- `ci`: run the ownership-layer shards for `compile`, `manifest`, `kernel`, `adapters`, and `migration`
- `full`: run every shard, including `services` and `e2e`

## Shards

Available shards:

- `compile`
- `manifest`
- `kernel`
- `adapters`
- `services`
- `e2e`
- `migration`

The shards are intentionally not a second architecture model.
They are an execution layout for `framework::test`, with `services` kept as a practical operational shard even though the service proofs it runs span multiple layers.

## Useful Commands

```bash
nix run .#framework::test -- --list-shards
nix run .#framework::test -- --profile full --summary
nix run .#framework::test -- --shard compile --summary
nix run .#framework::test -- --shard services --summary
nix run .#framework::test -- --shard migration --summary
nix run .#framework::test -- --profile ci --summary-json /tmp/framework-test-summary.json
```

## Layer Intent

- `compile`: compile-time model, help, schema, documentation, and packaging proofs
- `manifest`: runtime manifest fixtures and handoff invariants between Nix and kernel
- `kernel`: kernel-owned workflow, validation, registry, summary, and run semantics
- `adapters`: thin shell and launcher process-edge behavior only
- `services`: service public surfaces, lifecycle, readiness, and extractability
- `e2e`: user-facing public behavior, install and upgrade flows, isolation, and self-host execution
- `migration`: deleted-seam guards and forward-only refactor regressions

## Canonical Sources

Use `tests/framework/default.nix` as the source of truth for:

- the complete registered check list
- the exact check names
- proof metadata such as `layer`, `proofKind`, and `canonical`

Use `tests/framework/framework-test-shards.nix` as the source of truth for:

- shard order
- shard membership
- `ci` versus `full` profile selection

## Reuse Guides

Use these checked-in guides when reviewing framework behavior or teaching an agent how the current framework surface works:

- `tests/framework/WORKFLOW_REUSE.md`
- `tests/framework/SERVICE_LIFECYCLE_API.md`

## Contract Migration Guard

`contract-migration-guard` is the repository policy gate for deleted seams.

It fails if:

- framework-owned CUE files or CUE references return
- `mkValidator.nix` or `run-registry.nix` returns
- deprecated kernel seams such as `validate-json`, `query-json`, or `json-length` return
- shell-owned `run-record`, `summary`, or `meta` sidecars return
- framework-owned `jq` returns under framework runtime or build-check paths
- machine output falls back to stdout scraping instead of `NIXFIED_MACHINE_OUTPUT_FILE`
- framework smokes fall back to inline Python responders
- deleted authored `nixfied.apps` surfaces return

Run it directly with:

```bash
nix run .#framework::test -- --shard migration --summary
nix build .#checks.$(nix eval --impure --raw --expr builtins.currentSystem).contract-migration-guard
```

## Output Contract

All framework-facing command output must remain ASCII and prefix-based:

- `INFO:`
- `WARN:`
- `ERROR:`
- `OK:`
- `SKIP:`
