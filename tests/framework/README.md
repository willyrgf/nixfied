# Framework Tests

This directory documents the native framework test surface.

## Primary command

Run from repository root:

```bash
nix run .#framework::test
```

Path-based reference for untracked changes:

```bash
nix run path:.#framework::test
```

## Behavior

`framework::test` is a first-class task defined in `nixfied/project/module.nix`.
It runs validation shards with stable log prefixes and supports shard-level parallelism.

The authoritative flake-check registry lives in `tests/framework/default.nix`.
This README is an overview, not the canonical full check list.

Available shards:
- `flake-check`
- `help`
- `workflow-test`
- `workflow-ci`
- `isolation`
- `self-host`

## Useful options

```bash
nix run .#framework::test -- --list-shards
nix run .#framework::test -- --shard flake-check
nix run .#framework::test -- --shard self-host
nix run .#framework::test -- --mode env --summary
nix run .#framework::test -- --summary-json /tmp/framework-test-summary.json
```

## Flake checks (canonical)

Deterministic checks live in `tests/framework/` and run via `nix flake check path:.`.

Use `tests/framework/default.nix` as the source of truth for:
- the complete registered check list
- the exact check names
- the import path for each check

The current surface is organized around:
- model and compiler determinism
- executor, env sandbox, and shell/runtime contracts
- registry, orchestrator, and summary behavior
- operations, readiness, and service observability
- install/upgrade wrapper flows
- ephemeral execution and isolation behavior
- framework CLI and self-host smoke coverage

Shared shell helpers live in `tests/framework/lib/harness.nix`.
High-complexity operations smokes still have room for more harness extraction, but the harness is active and should be preferred over ad hoc duplication.

## Output contract

All framework-facing command output must remain ASCII and prefix-based:
- `INFO:`
- `WARN:`
- `ERROR:`
- `OK:`
- `SKIP:`
