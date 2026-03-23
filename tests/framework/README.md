# Framework Tests

This directory documents the native framework test surface.

## Primary command

Run from repository root:

```bash
nix run .#framework::test
```

## Behavior

`framework::test` is a first-class framework preset task defined in `nixfied/framework/presets/framework-test.nix`.
By default it runs all framework shards, including the full registered framework
suite via `nix flake check .`.
Named shards remain available for focused debugging with stable log prefixes.

The authoritative flake-check registry lives in `tests/framework/default.nix`.
This README is an overview, not the canonical full check list.

Available shards:
- `flake-check`
- `launcher-pruning`
- `help`
- `workflow-ci`
- `services`
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

Deterministic checks live in `tests/framework/` and run via `nix flake check .`.
The `framework::test` default path runs every shard, with `flake-check`
covering the full `nix flake check .` suite and the remaining shards exercising
targeted framework runner surfaces. Named shards are still useful for focused
debugging, such as `launcher-pruning` for launcher-driven compile-time service
exclusion.

## Launcher Pruning Proof

`launcher-pruning` is the canonical executed proof for selector-aware launcher
behavior, disabled-service runtime-surface filtering, and public help
fast-paths.

Use it when a downstream project needs confidence that:

- `SKIP_<SERVICE>` launcher sugar is converted into compile-time graph exclusion
- disabled services do not leak runtime hook env or generated service apps into
  unrelated surfaces
- `nix run .#ci -- --help` and `SKIP_HELIOS=1 nix run .#ci -- --help` stay on
  the cheap public help path
- selector-driven recompilation prunes service-gated tasks while leaving
  unrelated tasks runnable

The proof is implemented by:

- `tests/framework/launcher-disabled-nginx-override.nix`
- `tests/framework/disabled-service-runtime-surface-smoke.nix`
- `tests/framework/launcher-skip-service-pruning-smoke.nix`
- `tests/framework/launcher-help-fast-path-smoke.nix`

Mechanism:

- the disabled-service smoke poisons nginx source selection while nginx remains
  disabled, then asserts that no `SVC_NGINX_*` hook env vars or `svc::nginx::*`
  apps are generated
- the pruning smoke enables Helios, verifies a Helios-gated task runs on the
  base graph, then reruns through `SKIP_HELIOS=1` and requires that the gated
  task disappear while a control task still runs
- the help fast-path smoke proves both the direct launcher path and the exact
  public `nix run .#ci -- --help` surfaces avoid `nixfied-selected-app-*`
  when no selectors are active

That combination proves the framework no longer leaks disabled services into
runtime surface generation, keeps public help cheap, and still recompiles a
pruned graph when selectors are actually in play.

Run it directly with:

```bash
nix run .#framework::test -- --shard launcher-pruning --summary
```

Use `tests/framework/default.nix` as the source of truth for:
- the complete registered check list
- the exact check names
- the import path for each check

## Contract Migration Guard

`contract-migration-guard` is the repository policy gate for the CUE contract
migration.

It fails if:

- framework-owned Python helpers return under `nixfied/framework/core/`
- any framework source under `nixfied/framework/` uses `jq` outside the explicit allowlist for adapter, validation, or build-check roles
- machine output transport falls back to stdout scraping instead of the
  declared `NIXFIED_MACHINE_OUTPUT_FILE` channel

That guard complements the runtime contract checks. The architecture is now:

- contract definitions in `nixfied/contracts/`
- generated validator bundles in `nixfied/framework/contracts/`
- explicit payload-file machine output transport
- compile-time introspection bundles with a thin runtime selector
- repo-wide `jq` policy enforced by allowlist rather than by migrated-path convention

Run it directly with:

```bash
nix run .#framework::test -- --shard flake-check --summary
nix build .#checks.$(nix eval --impure --raw --expr builtins.currentSystem).contract-migration-guard
```

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
