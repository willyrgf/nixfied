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

Deterministic checks live in `tests/framework/` and run via `nix flake check path:.`:
- `model-hash`
- `cross-machine-hash`
- `scheduler-order`
- `help-snapshot`
- `registry-replay`
- `compiler-validation`
- `executor-contract`
- `env-sandbox-contract`
- `operations-contract`
- `registry-events-contract`
- `log-prefix-contract`
- `parallel-runner-smoke`
- `parallel-worker-cap-smoke`
- `parallel-worker-cap-invalid-smoke`
- `ci-mode-matrix-smoke`
- `task-hooks-smoke`
- `framework-test-cli-contract-smoke`
- `framework-selfhost-contract`
- `framework-install-vendor-smoke`
- `framework-install-thin-smoke`
- `framework-upgrade-preserve-smoke`
- `orchestrator-lifecycle-contract`
- `orchestrator-stop-controls-smoke`
- `workflow-lifecycle-smoke`
- `summary-json-smoke`

## Output contract

All framework-facing command output must remain ASCII and prefix-based:
- `INFO:`
- `WARN:`
- `ERROR:`
- `OK:`
- `SKIP:`
