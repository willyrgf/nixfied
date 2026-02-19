# Framework Tests (v2)

This directory documents the v2-native framework test surface.

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

`framework::test` is a first-class v2 task defined in `nixfied/project/module.nix`.
It runs deterministic validation shards with stable log prefixes.

Available shards:
- `flake-check`
- `help`
- `workflow-test`
- `workflow-ci`
- `isolation` (runs when `FRAMEWORK_ISOLATION=1` or explicitly selected)

## Useful options

```bash
nix run .#framework::test -- --list-shards
nix run .#framework::test -- --shard flake-check
nix run .#framework::test -- --mode env --summary
nix run .#framework::test -- --summary-json /tmp/framework-test-summary.json
```

## Flake checks (canonical)

Deterministic v2 checks live in `tests/framework/v2/` and run via `nix flake check path:.`:
- `v2-model-hash`
- `v2-cross-machine-hash`
- `v2-scheduler-order`
- `v2-help-snapshot`
- `v2-registry-replay`
- `v2-compiler-validation`
- `v2-executor-contract`
- `v2-env-sandbox-contract`
- `v2-registry-events-contract`
- `v2-log-prefix-contract`

## Output contract

All framework-facing command output must remain ASCII and prefix-based:
- `INFO:`
- `WARN:`
- `ERROR:`
- `OK:`
- `SKIP:`
