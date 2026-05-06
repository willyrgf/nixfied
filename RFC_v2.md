# RFC v2: Problem-First Nixfied Rewrite

Date: 2026-05-06
Status: Draft

## Problem

Modern projects are not one program. They are small systems: frontend, API, workers, databases, queues, object storage, migrations, test harnesses, infra scripts, and CI jobs, often written in different languages and owned by different tools.

The operational behavior of those systems is usually scattered across `flake.nix`, shell scripts, package-manager scripts, CI YAML, Docker compose files, service wrappers, env files, port conventions, and local tribal knowledge. That is only half the problem.

The deeper problem is that projects do not have a clean deterministic way to run multiple environments, or multiple copies of the same environment, while keeping services, workflows, state, logs, ports, artifacts, and cleanup under one process-aware authority.

Without first-class environment, slot, state, and registry concepts:

- `dev`, `test`, `ci`, preview, and prod-like runs collide.
- Two copies of the same environment cannot run predictably.
- Ports and mutable state leak between runs.
- Services are started by one script and stopped by another, if they are stopped at all.
- Workflows cannot reliably know which service instance they own.
- CI failures leave weak evidence about what ran, what was canceled, and what survived.
- Polyglot codebases need too much glue to coordinate simple local and CI execution.

Nixfied v2 exists to solve that problem.

## Product Statement

Nixfied v2 is a deterministic runtime coordinator for project-shaped systems.

It should compile project intent into a typed model, then run that model through one runtime that can start, inspect, record, and clean up the full execution graph across environments, slots, services, workflows, and codebases.

Nixfied is not primarily a task runner, service template collection, CI wrapper, or Nix scaffold. Those may exist as surfaces or adapters, but the product is the deterministic execution model.

## Core Concepts

- **Model:** the compiled contract for what exists and how it can run.
- **Environment:** the named mode of operation, such as `dev`, `test`, `ci`, preview, or prod-like.
- **Slot:** a deterministic parallel instance of an environment.
- **State:** all mutable roots derived from project, environment, slot, and run identity.
- **Registry:** the process-first source of truth for what is running, what happened, who owns it, and how to inspect or stop it.
- **Service:** a long-lived process with lifecycle, readiness, health, logs, state, and ownership.
- **Workflow:** a dependency graph of tasks and service phases with artifacts, summaries, cancellation, and cleanup.
- **Adapter:** language-, service-, or tool-specific implementation behind a stable model/runtime contract.

## Required Capabilities

1. **Deterministic env and slot execution.** The framework must derive ports, state roots, registry paths, artifact paths, and service roots from explicit project/env/slot/run identity. Multiple slots of the same env must run without collision.

2. **Process-first registry.** The runtime must record services, workflows, tasks, child processes, lifecycle state, readiness, health, cancellation, artifacts, summaries, and stop ownership in one inspectable registry.

3. **Service and workflow lifecycle ownership.** A run must know which services it needs, which instances it owns or reuses, when they are ready, how they are checked, and how they are stopped or left running by policy.

4. **Polyglot project coordination.** The framework must coordinate many small codebases and toolchains without forcing one language, package manager, service runner, or repo shape.

5. **Typed model and deterministic compiler.** Project intent must be expressed as typed data, compiled into a canonical model, validated before execution, and exposed through stable introspection and schemas.

6. **One runtime authority.** Invocation-time execution, process ownership, run records, summaries, artifacts, cancellation, cleanup, and env sandboxing must belong to one runtime path, not scattered shell helpers.

7. **Optional service adapters.** Core defines the lifecycle contract and dispatcher. Specific adapters such as postgres, nginx, minio, reth, and helios are optional modules, not framework identity.

8. **Installable downstream wrapper.** A project should be able to adopt or upgrade the framework without vendoring the framework into project-owned files or losing project-owned configuration.

9. **Proof through real execution.** The primary proof is a canonical workspace that exercises envs, slots, services, workflows, state, registry, cancellation, summaries, and install/upgrade through public surfaces.

## Design Consequences

The public API should be derived from the execution model, not from the current command list.

V2 should expose only the surfaces needed to:

- discover the compiled model
- validate env/slot/state assumptions
- run tasks and workflows
- start/check/stop service lifecycle operations
- inspect registry state
- stop owned processes
- install or upgrade a downstream wrapper

Every feature must answer:

1. What problem does it solve in deterministic multi-env execution?
2. What model field owns it?
3. What compiler pass validates it?
4. What runtime path executes or observes it?
5. What proof demonstrates it?

If a feature cannot answer those questions, it is not core.

## Non-Goals

- No broad downstream project template as framework core.
- No required service adapter for a minimal project.
- No duplicate command families for the same lifecycle action.
- No shell-owned semantic sidecars for graph, registry, summary, or validation behavior.
- No checked-in vendored framework fixture.
- No tests that encode historical structure instead of product guarantees.

## Success Criteria

A downstream project can define multiple codebases, tasks, workflows, services, environments, slots, machine outputs, and state policy in one typed model, then deterministically run and inspect any environment or slot through one stable runtime with isolated state, managed lifecycle, clean cancellation, durable registry history, and reproducible summaries.
