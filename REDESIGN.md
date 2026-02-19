# Redesign Notes (v2)

## Status

The model-first v2 redesign is complete. The temporary planning document `REFACTOR_ARCHITECTURE.md` has been retired.

## What Changed

- Replaced ad hoc composition with typed module evaluation (`lib.evalModules`).
- Consolidated command/workflow definitions in `nixfied/project/module.nix`.
- Added explicit compiler passes that produce `nixfiedModel.v2`.
- Standardized on canonical Nix hashing (`stateHash = sha256(toCanonicalNix(model))`).
- Unified execution under dispatcher + deterministic executor.
- Rebuilt registry as strict append-only NDJSON with deterministic replay.
- Promoted model-generated help/docs/app surfaces as the external contract.

## Operator-Facing Impact

- Legacy hook-driven command surfaces are no longer the primary runtime API.
- Core interfaces are flake apps (`nix run .#<cmd>`) and dispatcher surfaces (`run-task`, `run-workflow`).
- Determinism and contract behavior are enforced by `tests/framework/v2/`.

## Ongoing Documentation Sources

- `README.md`: entrypoints and stable interfaces.
- `ARCHITECTURE.md`: high-level architecture.
- `docs/DETAILED.md`: detailed model, runtime, and registry contracts.
- `docs/repo-index.json` and `docs/repo-map.md`: repository discovery map.
