# RFC v2: Nixfied Rewrite One-Pager

Date: 2026-05-06
Status: Draft

## Thesis

Nixfied v2 should be a small model-first Nix runtime framework. The project is currently carrying too many concerns at once: framework kernel, downstream project template, service adapters, proof harness, install wrapper, and historical compatibility surface. V2 should reduce this to one product kernel plus optional adapters.

The product is: a typed project model, a deterministic compiler, one runtime engine, stable flake apps, and optional service adapters behind one ABI. Anything else must be classified as adapter, proof, or documentation.

## Must-Have Core

1. **Canonical model.** One typed `nixfiedModel` owns identity, env/slot policy, ports, tasks, workflows, service requirements, service adapters, machine outputs, state policy, public app metadata, and feature metadata. It must be deterministic, hashable, introspectable, and cheap to evaluate. Heavy runtime artifacts may be derived from it, but may not redefine semantics.

2. **Compiler-owned semantics.** The compiler owns module resolution, runtime normalization, task/workflow graph compilation, service requirement validation, service ABI publication, machine-output contract validation, generated views, schemas, and introspection. Runtime scripts must not invent a second graph, service policy, validation contract, or summary contract.

3. **One runtime engine.** Runtime owns invocation-time behavior: `run-task`, `run-workflow`, `run-workflow-parallel`, `runs`, `stop-run`, `stop-all-runs`, artifact placement, summaries, registry append/replay/status, process-group cleanup, ephemeral execution, and env sandboxing. The Rust kernel owns durable semantics where shell is fragile: run ids, workflow transitions, records, validation, summary composition, registry replay, probes, and machine-output envelopes.

4. **Small public flake API.** Keep the public app set bounded:
   - discovery: `help`, `docs`, `features`, `introspect`, `schema`, `stateHash`
   - execution: `run-task`, `run-workflow`, `run-workflow-parallel`
   - state/control: `runs`, `stop-run`, `stop-all-runs`
   - operations: `validate-env`, `ports`, `check-ports`, `ready`, `health`
   - packaging: `framework::install`, `framework::upgrade`
   - project aliases: `dev`, `test`, `ci`, `build`, `check`, `format`
   - services: `svc::<service>::<op>` only when a service is enabled

5. **Single service ABI.** Core defines the service contract and dispatcher. Service adapters are optional modules. The core service contract covers enable/config/source selection, lifecycle classes (`setup`, `start`, `status`, `ready`, `health`, `stop`), operation metadata, command APIs, and `svc::<service>::<op>` publication. Postgres, nginx, minio, reth, and helios are adapters, not framework identity.

6. **State and isolation guarantees.** V2 must preserve workspace-scoped state, deterministic env/slot port derivation, isolated runtime/registry/service/artifact roots, append-only registry events, stable summaries, ephemeral source materialization with budget/retention policy, and no leaked secrets or host mutable state in managed roots.

7. **Thin install/upgrade.** Install creates a runnable downstream wrapper. Upgrade refreshes framework-owned files and preserves project-owned files unless explicitly reset. No checked-in proof fixture may become a vendored framework snapshot.

8. **Lean proof system.** Keep compile proofs for deterministic model/schema/feature/service ABI publication, kernel unit tests for runtime semantics, one canonical proof workspace for public runtime behavior, and adapter-specific tests only for adapter-specific guarantees. Delete or rewrite tests that exist only because behavior is scattered.

## Public API Rule

No new public command family without an RFC. Removed families stay removed: `svcset::*`, `services-*`, `SVC_*`, and `SKIP_*`.

## Feature Admission Rule

Every V2 feature must answer four questions before implementation:

1. What model field owns it?
2. What compiler pass validates it?
3. What public app or library API exposes it?
4. What proof demonstrates it?

If a feature cannot answer all four, it is not core.

## Non-Goals

- No broad downstream project template as framework core.
- No duplicate public command families.
- No shell-owned semantic sidecars.
- No required service adapter for a minimal project.
- No checked-in vendored framework fixture.
- No tests that encode historical structure instead of product guarantees.

## Success Criteria

A downstream project can define tasks, workflows, env/slot policy, optional services, and machine outputs through a small typed model, then run all public commands through one stable flake interface with deterministic state, summaries, registry history, and clean process behavior.
