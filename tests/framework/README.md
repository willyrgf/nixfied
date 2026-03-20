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

`framework::test` is a first-class framework preset task defined in `nixfied/framework/presets/framework-test.nix`.
It runs validation shards with stable log prefixes and supports shard-level parallelism.

The authoritative flake-check registry lives in `tests/framework/default.nix`.
This README is an overview, not the canonical full check list.

Available shards:
- `flake-check`
- `launcher-pruning`
- `help`
- `workflow-ci`
- `isolation`
- `self-host`

## Useful options

```bash
nix run .#framework::test -- --list-shards
nix run .#framework::test -- --shard flake-check
nix run .#framework::test -- --shard launcher-pruning
nix run .#framework::test -- --shard self-host
nix run .#framework::test -- --mode env --summary
nix run .#framework::test -- --summary-json /tmp/framework-test-summary.json
```

## Reuse Guides

Use these checked-in guides when reviewing framework behavior or teaching an
agent how the current framework surface works:

- `tests/framework/WORKFLOW_REUSE.md`
- `tests/framework/SERVICE_LIFECYCLE_API.md`

## Flake checks (canonical)

Deterministic checks live in `tests/framework/` and run via `nix flake check path:.`.
The `framework::test` `flake-check` shard intentionally uses `nix flake check path:. --no-build` so the framework harness validates the check graph without recursively rebuilding the same workflow-heavy checks that other shards already exercise.
Checks that need execution inside the framework harness should be wired into a dedicated shard, such as `launcher-pruning` for launcher-driven compile-time service exclusion.

## Launcher Pruning Proof

`launcher-pruning` is the canonical executed proof that launcher-driven service
exclusion happens before the selected app evaluates an excluded service branch.

Use it when a downstream project needs confidence that:

- `SKIP_<SERVICE>` launcher sugar is converted into compile-time graph exclusion
- the selected app does not evaluate the excluded service's project branch
- excluded-service helper generation and package selection do not leak into the
  surviving launcher path

The proof is implemented by:

- `tests/framework/launcher-skip-helios-override.nix`
- `tests/framework/launcher-skip-service-pruning-smoke.nix`

Mechanism:

- the override replaces the Helios source with a poison package that throws
  `"helios evaluated unexpectedly"` as soon as Helios source selection is
  touched
- the smoke first runs the launcher without `SKIP_HELIOS` and requires that
  exact failure
- it then reruns the same launcher with `SKIP_HELIOS=1` and requires success
  while also asserting that the poison message never appears

That combination proves more than runtime skip. It proves the launcher applies
compile-time exclusion before selected-app evaluation reaches the Helios branch.

Run it directly with:

```bash
nix run .#framework::test -- --shard launcher-pruning --summary
```

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
- SKIP service behavior and service-skip dependency semantics

Shared shell helpers live in `tests/framework/lib/harness.nix`.
High-complexity operations smokes still have room for more harness extraction, but the harness is active and should be preferred over ad hoc duplication.

## Output contract

All framework-facing command output must remain ASCII and prefix-based:
- `INFO:`
- `WARN:`
- `ERROR:`
- `OK:`
- `SKIP:`
