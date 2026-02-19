# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

Nixfied is a model-first v2 framework. Typed Nix modules are compiled into `nixfiedModel.v2`, and command/help/docs views are generated from that model.

## Build and Development Commands

```bash
nix run .#help
nix run .#dev
nix run .#test
nix run .#build
nix run .#check
nix run .#format
nix run .#ci -- --summary
nix develop
```

## Operational and Introspection Commands

```bash
nix run .#validate-env
nix run .#test-isolation
nix run .#ports
nix run .#check-ports

nix run .#model
nix run .#stateHash
nix run .#tasks
nix run .#task::<id>
nix run .#schema
```

Dispatcher surfaces:

```bash
nix run .#run-task -- <task-id> [-- ...]
nix run .#run-workflow -- <workflow-id> [-- ...]
```

Framework-only commands (require `.workspace` marker):

```bash
nix run .#framework::test
nix run .#framework::install
```

## Testing

```bash
nix run .#framework::test
nix run path:.#framework::test
nix run .#framework::test -- --list-shards
nix run .#framework::test -- --shard help
nix flake check path:.
```

Primary deterministic checks live in `tests/framework/v2/` and include model hash, scheduler order, help snapshot, registry replay, and runtime contract gates.

## Architecture

### Current Layout

```text
flake.nix
  nixfied/
    project/
      conf.nix
      module.nix
    modules/
      core.nix
      runtime.nix
      tasks.nix
      workflows.nix
      operations.nix
      services/*.nix
      profiles/*.nix
    compiler/*.nix
    runner/*.nix
    registry/*.nix
    lib/*.nix
    install/wrapper-flake.nix
    schemas/*.json
  docs/
  tests/framework/
```

### Model Pipeline

`modules -> resolved config -> compile-* passes -> finalize-model -> nixfiedModel.v2 -> generated views/apps`

Compiler passes are in `nixfied/compiler/`:
- `resolve-modules`
- `normalize-runtime`
- `compile-services`
- `compile-tasks`
- `compile-workflows`
- `compile-views`
- `finalize-model`

### Project Command Model

Commands are modeled as tasks/workflows in `nixfied/project/module.nix`:

```nix
config.nixfied.tasks.my-task = {
  id = "task.my-task";
  summary = "My task";
  runner.type = "shell";
  runner.command = ''
    set -euo pipefail
    echo "INFO: run my task"
  '';
  ui.app = {
    expose = true;
    name = "my-task";
    category = "core";
  };
};
```

```nix
config.nixfied.workflows.my-workflow = {
  id = "workflow.my-workflow";
  summary = "My workflow";
  units.my-unit.taskId = "task.my-task";
};
```

## Conventions

- Keep output ASCII and stable-prefix based: `INFO:`, `WARN:`, `ERROR:`, `OK:`, `SKIP:`.
- Keep behavior deterministic and idempotent; fail fast on invalid config.
- Prefer `set -euo pipefail` in shell task bodies.
- Keep command/help output contracts stable and update help snapshots when command surfaces change.
- Use `nix run path:.#<app>` while validating untracked local changes.
