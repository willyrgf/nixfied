# Redesign and Migration Context

## Summary

Nixfied was redesigned around a model-first architecture.

Task/workflow definitions, help output, and runtime surfaces are now derived from a compiled `nixfiedModel` instead of ad hoc script wiring.

## Why the Redesign Happened

Primary goals:

- deterministic behavior across machines
- explicit typed module contracts
- stable command surfaces generated from model metadata
- clear runtime and registry contracts for tooling and testing

## What Changed

- Centralized configuration in typed module options (`nixfied/modules/`).
- Explicit compiler pass pipeline (`nixfied/compiler/`).
- Canonical state hashing from Nix rendering (`toCanonicalNix`).
- Process-first runtime (`dispatcher -> orchestrator -> executor`) for tasks/workflows (`nixfied/runner/`).
- Strict NDJSON event registry with replay/snapshot support (`nixfied/registry/`).
- Determinism-focused validation in `tests/framework/`.

## Current Design Baseline

The active baseline is:

- model-generated app surfaces (`nix run .#help`)
- model-derived introspection apps (`model`, `stateHash`, `tasks`, `schema`)
- CI/workflow composition through `workflow.ci.*`
- strict CLI prefix conventions (`INFO:`, `WARN:`, `ERROR:`, `OK:`, `SKIP:`)

## Migration Notes for Existing Projects

When migrating custom project logic:

1. Move project-specific settings into `nixfied/project/conf.nix`.
2. Define or refine tasks/workflows in `nixfied/project/module.nix`.
3. Keep exposed app names lowercase in `ui.app.name`.
4. Validate surface and contracts with:
   - `nix run .#help`
   - `nix run .#framework::test`
   - `nix run .#ci -- --summary`

## Non-Goals

- Reintroducing legacy fixture harness wiring as a primary control plane.
- Restoring non-deterministic command behavior hidden behind shell defaults.
