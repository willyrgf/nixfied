# Nixfied Refactor Architecture (Nix-Native, No Compatibility Layer)

## 1. Goals

This document defines a full rewrite architecture for Nixfied with breaking changes allowed.

Primary goals:

- Deterministic evaluation and deterministic execution planning.
- Composable and reusable framework via Nix modules and a library API.
- Single canonical, typed Nix model as source of truth.
- No JSON in the core pipeline.
- Apps/help/docs are generated views, not primary interfaces.

Non-goals:

- Backward compatibility with current contracts, hooks, or project schema.
- Incremental migration inside one runtime path.

## 2. North Star

Pipeline:

`modules -> resolved config (evalModules) -> expanded typed Nix model -> task/workflow graph -> deterministic executor -> generated views`

Canonical source of truth is a versioned Nix attrset: `nixfiedModel.v2`.

## 3. Hard Breaking Changes

1. Remove env-hook integration as primary API (`SVC_*`, `SUPERVISOR_*`, etc.).
2. Remove current app/service contract versions and command-class coupling.
3. Remove recursive attrset merge composition in `nixfied/project/default.nix`.
4. Remove special-case generators as architecture (`core/module-apps/ci` split).
5. Remove vendored framework as architecture default; use library input model.

## 4. Canonical Model

`nixfiedModel.v2` is a typed attrset emitted by `lib.mkNixfied`.

```nix
{
  schema = {
    kind = "nixfied-model";
    version = 2;
  };

  identity = {
    projectId = "my-project";
    system = "x86_64-linux";
    evalHash = "<sha256>";
  };

  runtime = {
    slot = {
      var = "NIX_ENV";
      default = 0;
      max = 9;
      stride = 1;
    };
    env = {
      var = "PROJECT_ENV";
      names = [ "dev" "test" "prod" ];
      offsets = { dev = 10; test = 20; prod = 0; };
      default = "dev";
    };
    logging = {
      levelDefault = "info";
      outputDefault = "stdout";
    };
    primitives = {
      version = 1;
      defs = {
        LOG_LEVEL = {
          type = "enum";
          values = [ "error" "warn" "info" "debug" "trace" ];
          default = "info";
          aliases = [ "NIXFIED_LOG_LEVEL" ];
        };
        OUTPUT_MODE = {
          type = "enum";
          values = [ "stdout" "logs" "both" ];
          default = "stdout";
          aliases = [ "NIXFIED_OUTPUT_MODE" ];
        };
      };
    };
  };

  services = { };   # Derived service specs
  tasks = { };      # TaskSpec map by id
  workflows = { };  # WorkflowSpec map by id

  views = {
    apps = { };      # Generated app definitions
    help = { };      # Generated help docs model
    docs = { };      # Generated docs model
  };

  state = {
    registry = {
      schemaVersion = 1;
      root = "/tmp/nixfied-runtime/my-project";
    };
    artifacts = {
      root = "/tmp/ci-artifacts";
    };
  };
}
```

## 5. Determinism Rules

Determinism must be defined at model and plan levels.

### 5.1 Evaluation determinism

- All order-sensitive structures must be explicit lists, not implicit attrset traversal.
- Attrsets emitted for hashing must be canonicalized by sorted keys.
- IDs are generated from stable canonical inputs only.
- No time/random data may enter `nixfiedModel`.

### 5.2 Planning determinism

- Workflow plan expansion order is stable and explicit.
- Ready-queue tie-break is lexicographic by `task.id`.
- Lock arbitration tie-break is lexicographic by `task.id`.
- Retries/backoff are deterministic functions of attempt number and config.

### 5.3 Execution determinism

- Runtime boundary is hermetic and declared.
- Environment is sanitized and allowlisted.
- PATH is built from declared runtime inputs only.
- Locale/timezone/umask/cwd are fixed by runner defaults unless explicitly overridden.

## 6. Canonical Hashing

Canonical hash is based on a canonical Nix rendering of the model, not JSON.

```nix
# Pseudocode
stateHash = sha256 (toCanonicalNix nixfiedModel);
```

`toCanonicalNix` requirements:

- Sort keys recursively for attrsets.
- Preserve list ordering exactly.
- Render primitives with stable representation.
- Reject function values in canonicalized subtrees.

## 7. Module System Design

Use Nix module system (`lib.evalModules`) for all project and framework configuration.

### 7.1 Module layers

1. Framework base module.
2. Framework service modules.
3. Framework profile modules.
4. Project module(s).
5. Local overrides.

### 7.2 Required shape

```nix
{
  imports = [
    nixfied.modules.core
    nixfied.modules.services.postgres
    nixfied.profiles.webapp
    ./nixfied/project/module.nix
  ];
}
```

### 7.3 Rule

No recursive attrset ad-hoc merge. All config flows through typed options.

### 7.4 Conflict resolution policy

Module merge collisions must follow explicit rules:

- Scalar collisions:
  - If two definitions for the same scalar option are not equal after normalization, fail evaluation.
  - No implicit "last writer wins" behavior is allowed.
- Identity collisions (task IDs, workflow IDs, service IDs, named ports):
  - If canonical values are byte-identical, allow.
  - If values differ, fail evaluation and print both source locations.
- List collisions:
  - Must use explicit ordering semantics (`mkBefore`, `mkAfter`, `mkOrder`).
  - Compiler emits a fully expanded final order into the model; no hidden append behavior.
- Attrset/map collisions:
  - Merge recursively only when the option type explicitly allows it.
  - Otherwise fail evaluation.

## 8. TaskSpec Schema (Draft)

This is the canonical internal execution primitive.

### 8.1 Concept

Every executable unit is a task:

- command
- service operation
- supervisor operation
- utility
- CI step
- workflow helper

### 8.2 TaskSpec type

```nix
TaskSpec = {
  id = "task.dev.start";              # stable, unique, deterministic
  kind = "command";                   # enum
  summary = "Start local dev stack";
  description = "Longer description";
  tags = [ "dev" "local" ];

  runner = {
    type = "shell";                   # shell | derivation | workflowRef
    command = ''
      echo "hello"
    '';
    package = null;                   # derivation when type=derivation
    workflowId = null;                # when type=workflowRef
  };

  contract = {
    version = 1;
    input = {
      args = {
        parser = "typed";             # typed | passthrough | json
        allowUnknown = false;
        spec = [ ];
      };
      env = {
        schemaRef = "runtimePrimitives.v1";
        extra = [ ];
      };
    };
    output = {
      format = "text";                # text | kv | json | ndjson
      channels = "stdout";            # stdout | logs | both
      keys = [ ];
    };
    behavior = {
      idempotent = false;
      effects = [ "writes-state" "starts-daemon" ];
      timeoutSec = 0;                 # 0 = no timeout
    };
    errors = {
      codes = {
        generic = 1;
        usage = 2;
        precondition = 3;
      };
    };
  };

  runtime = {
    slotEnv = "required";             # required | optional | disabled
    workdir = "projectRoot";          # projectRoot | stateRoot | custom
    customWorkdir = null;
    hermetic = true;
    runtimeInputs = [ ];
    passThroughEnv = [ "HOME" ];
    env = { };
    umask = "022";
    locale = "C.UTF-8";
    timezone = "UTC";
  };

  scheduling = {
    locks = [ "slot:0:dev" ];
    maxAttempts = 1;
    retryBackoffSec = [ ];
    priority = 100;                   # tie-break still by id
  };

  deps = {
    needs = [ ];                      # explicit task IDs
    softNeeds = [ ];
  };

  produces = {
    artifacts = [ "logs/dev.log" ];
    stateKeys = [ ];
  };

  ui = {
    app = {
      expose = true;
      name = "dev";
      category = "core";
      usage = [ "nix run .#dev" ];
      examples = [ "PROJECT_ENV=dev NIX_ENV=0 nix run .#dev" ];
    };
  };
};
```

### 8.3 Task kinds

Allowed `kind` values:

- `command`
- `service-op`
- `supervisor-op`
- `utility`
- `ci-step`
- `workflow`
- `internal`

### 8.4 Option schema (Nix module draft)

```nix
{ lib, ... }:
let
  t = lib.types;
  argSpec = t.submodule {
    options = {
      name = lib.mkOption { type = t.str; };
      kind = lib.mkOption { type = t.enum [ "flag" "option" "positional" ]; };
      type = lib.mkOption { type = t.enum [ "string" "int" "bool" "enum" "pathAbs" "pathRel" "json" "durationSec" "port" ]; default = "string"; };
      long = lib.mkOption { type = t.nullOr t.str; default = null; };
      short = lib.mkOption { type = t.nullOr t.str; default = null; };
      required = lib.mkOption { type = t.bool; default = false; };
      values = lib.mkOption { type = t.listOf t.str; default = [ ]; };
      min = lib.mkOption { type = t.nullOr t.int; default = null; };
      max = lib.mkOption { type = t.nullOr t.int; default = null; };
      description = lib.mkOption { type = t.str; default = ""; };
    };
  };
  envSpec = t.submodule {
    options = {
      name = lib.mkOption { type = t.str; };
      type = lib.mkOption { type = t.enum [ "string" "int" "bool" "enum" "pathAbs" "pathRel" "json" "durationSec" "port" ]; default = "string"; };
      required = lib.mkOption { type = t.bool; default = false; };
      values = lib.mkOption { type = t.listOf t.str; default = [ ]; };
      default = lib.mkOption { type = t.nullOr (t.oneOf [ t.str t.int t.bool ]); default = null; };
      aliases = lib.mkOption { type = t.listOf t.str; default = [ ]; };
      sensitive = lib.mkOption { type = t.bool; default = false; };
      description = lib.mkOption { type = t.str; default = ""; };
    };
  };
in
{
  options.nixfied.tasks = lib.mkOption {
    type = t.attrsOf (t.submodule ({ name, ... }: {
      options = {
        id = lib.mkOption { type = t.str; default = name; };
        kind = lib.mkOption { type = t.enum [ "command" "service-op" "supervisor-op" "utility" "ci-step" "workflow" "internal" ]; };
        summary = lib.mkOption { type = t.str; };
        description = lib.mkOption { type = t.str; default = ""; };
        tags = lib.mkOption { type = t.listOf t.str; default = [ ]; };

        runner = {
          type = lib.mkOption { type = t.enum [ "shell" "derivation" "workflowRef" ]; default = "shell"; };
          command = lib.mkOption { type = t.lines; default = ""; };
          package = lib.mkOption { type = t.nullOr t.package; default = null; };
          workflowId = lib.mkOption { type = t.nullOr t.str; default = null; };
        };

        contract = {
          version = lib.mkOption { type = t.int; default = 1; };
          input.args.parser = lib.mkOption { type = t.enum [ "typed" "passthrough" "json" ]; default = "typed"; };
          input.args.allowUnknown = lib.mkOption { type = t.bool; default = false; };
          input.args.spec = lib.mkOption { type = t.listOf argSpec; default = [ ]; };
          input.env.schemaRef = lib.mkOption { type = t.str; default = "runtimePrimitives.v1"; };
          input.env.extra = lib.mkOption { type = t.listOf envSpec; default = [ ]; };
          output.format = lib.mkOption { type = t.enum [ "text" "kv" "json" "ndjson" ]; default = "text"; };
          output.channels = lib.mkOption { type = t.enum [ "stdout" "logs" "both" ]; default = "stdout"; };
          output.keys = lib.mkOption { type = t.listOf t.str; default = [ ]; };
          behavior.idempotent = lib.mkOption { type = t.bool; default = true; };
          behavior.effects = lib.mkOption { type = t.listOf (t.enum [ "none" "writes-state" "starts-daemon" "network" "reads-secrets" ]); default = [ "none" ]; };
          behavior.timeoutSec = lib.mkOption { type = t.int; default = 0; };
          errors.codes = lib.mkOption { type = t.attrsOf t.int; default = { generic = 1; usage = 2; precondition = 3; unavailable = 4; timeout = 5; }; };
        };

        runtime = {
          slotEnv = lib.mkOption { type = t.enum [ "required" "optional" "disabled" ]; default = "optional"; };
          workdir = lib.mkOption { type = t.enum [ "projectRoot" "stateRoot" "custom" ]; default = "projectRoot"; };
          customWorkdir = lib.mkOption { type = t.nullOr t.str; default = null; };
          hermetic = lib.mkOption { type = t.bool; default = true; };
          runtimeInputs = lib.mkOption { type = t.listOf t.package; default = [ ]; };
          passThroughEnv = lib.mkOption { type = t.listOf t.str; default = [ ]; };
          env = lib.mkOption { type = t.attrsOf (t.oneOf [ t.str t.int t.bool ]); default = { }; };
          umask = lib.mkOption { type = t.str; default = "022"; };
          locale = lib.mkOption { type = t.str; default = "C.UTF-8"; };
          timezone = lib.mkOption { type = t.str; default = "UTC"; };
        };

        scheduling = {
          locks = lib.mkOption { type = t.listOf t.str; default = [ ]; };
          maxAttempts = lib.mkOption { type = t.int; default = 1; };
          retryBackoffSec = lib.mkOption { type = t.listOf t.int; default = [ ]; };
          priority = lib.mkOption { type = t.int; default = 100; };
        };

        deps = {
          needs = lib.mkOption { type = t.listOf t.str; default = [ ]; };
          softNeeds = lib.mkOption { type = t.listOf t.str; default = [ ]; };
        };

        produces = {
          artifacts = lib.mkOption { type = t.listOf t.str; default = [ ]; };
          stateKeys = lib.mkOption { type = t.listOf t.str; default = [ ]; };
        };

        ui = {
          app.expose = lib.mkOption { type = t.bool; default = false; };
          app.name = lib.mkOption { type = t.str; default = name; };
          app.category = lib.mkOption { type = t.str; default = "core"; };
          app.usage = lib.mkOption { type = t.listOf t.str; default = [ ]; };
          app.examples = lib.mkOption { type = t.listOf t.str; default = [ ]; };
        };
      };
    }));
    default = { };
  };
}
```

## 9. WorkflowSpec Schema (Draft)

Workflows are deterministic DAGs over `TaskSpec`.

### 9.1 WorkflowSpec type

```nix
WorkflowSpec = {
  id = "workflow.ci.full";
  summary = "Full CI workflow";
  description = "Runs quality and tests in staged parallel groups.";

  mode = "ci";                        # ci | dev | test | build | check | format | custom
  maxWorkers = 4;

  # Canonical structure:
  # - Either DAG edges via units[].needs
  # - Or stages[] list (which compiles into deterministic DAG)
  units = {
    "quality" = {
      taskId = "task.check.quality";
      needs = [ ];
      locks = [ "repo:write" ];
      when = {
        envEquals = { PROJECT_ENV = "test"; };
        envPresent = [ ];
      };
      skipIfMissingEnv = [ ];
    };
    "tests" = {
      taskId = "task.test.unit";
      needs = [ "quality" ];
      locks = [ ];
      when = { envEquals = { }; envPresent = [ ]; };
      skipIfMissingEnv = [ "API_KEY" ];
    };
  };

  stages = [ [ "quality" ] [ "tests" ] ];

  setup = {
    tasks = [ "task.workflow.setup" ];
  };
  teardown = {
    tasks = [ "task.workflow.teardown" ];
    alwaysRun = true;
  };

  artifacts = {
    root = "/tmp/ci-artifacts";
    keepOnSuccess = false;
    keepOnFailure = true;
    writeSummary = true;
  };

  execution = {
    failFast = true;
    lockPolicy = "exclusive";         # exclusive | shared-aware
    emitRegistryEvents = true;
  };
};
```

### 9.2 Option schema (Nix module draft)

```nix
{ lib, ... }:
let
  t = lib.types;
  whenSpec = t.submodule {
    options = {
      envEquals = lib.mkOption { type = t.attrsOf (t.oneOf [ t.str t.int t.bool ]); default = { }; };
      envPresent = lib.mkOption { type = t.listOf t.str; default = [ ]; };
    };
  };
  workflowUnit = t.submodule {
    options = {
      taskId = lib.mkOption { type = t.str; };
      needs = lib.mkOption { type = t.listOf t.str; default = [ ]; };
      locks = lib.mkOption { type = t.listOf t.str; default = [ ]; };
      when = lib.mkOption { type = whenSpec; default = { }; };
      skipIfMissingEnv = lib.mkOption { type = t.listOf t.str; default = [ ]; };
    };
  };
in
{
  options.nixfied.workflows = lib.mkOption {
    type = t.attrsOf (t.submodule ({ name, ... }: {
      options = {
        id = lib.mkOption { type = t.str; default = name; };
        summary = lib.mkOption { type = t.str; };
        description = lib.mkOption { type = t.str; default = ""; };
        mode = lib.mkOption { type = t.enum [ "ci" "dev" "test" "build" "check" "format" "custom" ]; default = "custom"; };
        maxWorkers = lib.mkOption { type = t.int; default = 1; };

        units = lib.mkOption {
          type = t.attrsOf workflowUnit;
          default = { };
        };

        stages = lib.mkOption {
          type = t.listOf (t.listOf t.str);
          default = [ ];
        };

        setup.tasks = lib.mkOption { type = t.listOf t.str; default = [ ]; };
        teardown.tasks = lib.mkOption { type = t.listOf t.str; default = [ ]; };
        teardown.alwaysRun = lib.mkOption { type = t.bool; default = true; };

        artifacts = {
          root = lib.mkOption { type = t.str; default = "/tmp/ci-artifacts"; };
          keepOnSuccess = lib.mkOption { type = t.bool; default = false; };
          keepOnFailure = lib.mkOption { type = t.bool; default = true; };
          writeSummary = lib.mkOption { type = t.bool; default = true; };
        };

        execution = {
          failFast = lib.mkOption { type = t.bool; default = true; };
          lockPolicy = lib.mkOption { type = t.enum [ "exclusive" "shared-aware" ]; default = "exclusive"; };
          emitRegistryEvents = lib.mkOption { type = t.bool; default = true; };
        };
      };
    }));
    default = { };
  };
}
```

### 9.3 Workflow validation rules

- Exactly one authoring style:
  - explicit `units` DAG, or
  - `stages` (compiled into DAG).
- `units.<name>.taskId` must reference existing `nixfied.tasks`.
- `needs` must reference workflow unit names.
- No cycles.
- `stages` cannot duplicate unit names.
- All referenced lock tokens must match lock token grammar.

## 10. Compilation Passes

Compiler should be explicit multi-pass, each pass pure and testable.

1. `resolveModules`: `evalModules` -> resolved config.
2. `normalizeRuntime`: compute slot/env/ports/directories/runtime primitives.
3. `compileServices`: build normalized service specs.
4. `compileTasks`: emit all tasks with stable IDs.
5. `compileWorkflows`: resolve DAGs/stages and validate.
6. `compileViews`: generate app/help/docs views from tasks/workflows.
7. `finalizeModel`: assemble `nixfiedModel.v2` and compute `stateHash`.

## 11. Dispatcher and Executor

### 11.1 Dispatcher

Single stable dispatcher API:

- `run-task <task-id> [-- ...]`
- `run-workflow <workflow-id> [-- ...]`

No generated env-hook variables required.

### 11.2 Executor behavior

- Expand workflow to execution plan with deterministic ordering.
- Execute with stable scheduling and lock arbitration.
- Respect task runtime contract and hermetic boundary.
- Emit structured events to registry.

## 12. Hermetic Runtime Boundary

At task runtime:

- Reset env, then inject:
  - runtime primitives
  - task-declared env
  - allowlisted pass-through vars
- Build PATH from task `runtimeInputs`.
- Set deterministic defaults:
  - `LANG=C.UTF-8`
  - `LC_ALL=C.UTF-8`
  - `TZ=UTC`
  - `umask 022`
  - controlled `cwd`

Any ambient dependency is treated as a bug.

## 13. Registry and State

Registry is a versioned event log with strict on-disk serialization rules.

Event shape:

```nix
{
  schemaVersion = 1;
  seq = 42;                      # monotonic per registry root
  ts = "2026-02-19T18:00:00Z";   # ISO-8601 UTC
  runId = "run.<hash>";
  workflowId = "workflow.ci.full";
  taskId = "task.test.unit";
  state = "running";             # queued|running|passed|failed|canceled
  detail = { };
}
```

### 13.1 Event wire format (strict NDJSON)

- Events are stored at `$REGISTRY_ROOT/events.ndjson`.
- File format is strict NDJSON:
  - exactly one compact JSON object per line
  - UTF-8 encoding
  - newline-delimited, newline-terminated records
  - no pretty-print or multiline records
- Event keys are serialized in stable lexicographic order.
- `seq` is monotonic and contiguous per registry root, allocated under an exclusive writer lock.
- `ts` must be RFC3339 UTC with `Z` suffix.
- Numeric fields must be finite integers (no NaN/Infinity).
- Writers use append-only semantics; no in-place mutation or record rewrite.

### 13.2 Run ID formula and disambiguation policy

Base run ID must be deterministic:

```nix
runInput = concatStringsSep "|" [
  "run-id-v1"
  model.identity.evalHash
  (workflowId or "")
  (taskId or "")
  mode
  (toString slot)
  env
  normalizedArgsHash
  normalizedEnvHash
];
runBase = sha256 runInput;
runId = "run-${builtins.substring 0 24 runBase}";
```

Collision policy:

- If `runId` collides with an active run, append deterministic suffix `-cNNN` from a locked per-`runBase` counter.
- Random suffixes are not default behavior.
- Optional fallback random suffix (`-r<hex>`) is allowed only when:
  - deterministic counter allocation is unavailable, and
  - explicit config enables it.
- Any disambiguation suffix and reason is recorded in the first registry event.

### 13.3 State rules

- Append-only events.
- Snapshot/state views are derived.
- Replay must reconstruct state deterministically.

## 14. Library-First Flake API

Expose stable API:

```nix
nixfied.lib.mkNixfied {
  system = "x86_64-linux";
  projectRoot = ./.;
  projectModules = [ ./nixfied/project/module.nix ];
  extraModules = [ ];
}
```

Return:

```nix
{
  model;
  stateHash;
  tasks;
  workflows;
  apps;
  packages;
  checks;
  devShells;
}
```

`flake.nix` in consuming projects should be thin wrappers.

### 14.1 Introspection outputs contract

Expose introspection and tooling outputs as stable flake interfaces:

- `.#model`: print canonical `nixfiedModel.v2` representation.
- `.#stateHash`: print canonical model hash.
- `.#tasks`: list task IDs and summaries.
- `.#task::<id>`: print one fully resolved `TaskSpec`.
- `.#schema`: print derived JSON schemas for external tools.

### 14.2 Published JSON schemas (derived, external contract)

JSON is not the internal source of truth, but JSON schemas are published for external tooling interoperability.

- Schemas are generated from typed Nix definitions and versioned.
- Minimum published schemas:
  - task contract schema
  - workflow contract schema
  - model export schema
- `.#schema` must remain stable for a given schema version and be suitable for non-Nix clients.

### 14.3 Vendoring policy and installer mechanics

Vendoring remains optional, but is not the default architecture.

- Default installer behavior:
  - `framework::install` generates a thin wrapper flake.
  - Wrapper pins Nixfied as a flake input in `flake.lock`.
  - Project customization lives in local project modules.
- Optional vendoring mode:
  - `framework::install --vendor` copies framework sources into the repository.
  - Wrapper can point to vendored path input for local forks.
- Both modes must compile to the same canonical model semantics.

## 15. Generated Views

Views are generated from model only.

- `apps` view: task-dispatch wrappers for exposed tasks.
- `help` view: generated from task/workflow metadata.
- `docs` view: generated snippets for README/DETAILED docs.
- Operational command views (`validate-env`, `test-isolation`, `ports`, `check-ports`) must execute from compiled model data, never by re-deriving config ad hoc.

No view may re-derive config independently.

## 16. Determinism Test Gates

Required test classes:

1. Model hash stability for same inputs.
2. Model hash stability across different machines for the same commit and target `system`.
3. Task ID stability across evaluations.
4. Workflow expansion ordering stability.
5. Scheduler tie-break reproducibility under parallel load.
6. Help/docs snapshot stability from model.
7. Registry replay determinism.

Failure in any gate blocks merge.

## 17. Target Repository Restructure

Proposed structure:

```text
nixfied/
  lib/
    mkNixfied.nix
    canonical.nix
  modules/
    core.nix
    runtime.nix
    tasks.nix
    workflows.nix
    services/
      postgres.nix
      nginx.nix
      minio.nix
      reth.nix
      helios.nix
    profiles/
      webapp.nix
      eth.nix
  compiler/
    resolve-modules.nix
    normalize-runtime.nix
    compile-services.nix
    compile-tasks.nix
    compile-workflows.nix
    compile-views.nix
    finalize-model.nix
  runner/
    dispatcher.nix
    executor.nix
    env-sandbox.nix
  registry/
    events.nix
    snapshot.nix
    replay.nix
```

## 18. Full Refactor Execution Plan (No Compatibility)

1. Build module options and `evalModules` entrypoint.
2. Implement `TaskSpec` and `WorkflowSpec` typed options.
3. Implement compiler passes and `nixfiedModel.v2`.
4. Implement canonical hash and deterministic ID functions.
5. Replace app/help/docs generation to model-based views.
6. Replace CI-specific flow with universal workflow executor.
7. Replace hook integration with dispatcher.
8. Implement hermetic runtime policy enforcement.
9. Rebuild registry as versioned event log + replay.
10. Expose `mkNixfied` as primary API and simplify top-level `flake.nix`.
11. Add determinism gates and snapshot tests.
12. Remove old architecture files and dead paths.

## 19. Immediate Next Steps

1. Create module files for `options.nixfied.tasks` and `options.nixfied.workflows`.
2. Implement `lib/canonical.nix` with recursive stable renderer.
3. Add first compiler pass scaffold that emits empty `nixfiedModel.v2`.
4. Add one vertical slice:
   - one task
   - one workflow
   - one app generated from dispatcher
   - one determinism test.

## 20. Implementation Checklist (File-by-File Ownership and Sequence)

This section is the execution checklist for the refactor and is intended to be used as a live tracker.

### 20.1 Ownership lanes

Use fixed ownership lanes to avoid ambiguous responsibility:

- `lane.modules`: Nix options, profiles, service modules, merge/conflict policy.
- `lane.compiler`: model compilation passes, canonical hashing, deterministic IDs.
- `lane.runner`: dispatcher, workflow executor, hermetic runtime boundary.
- `lane.registry`: NDJSON event store, replay, run-id collision handling.
- `lane.surface`: flake outputs, introspection surfaces, installer/wrapper/vendoring behavior.
- `lane.cleanup`: deletion of old architecture files and dead paths.

### 20.2 Sequenced rollout matrix

| Seq | Phase | Owner | Create | Modify | Remove | Exit gate |
| --- | --- | --- | --- | --- | --- | --- |
| 0 | Bootstrap new architecture skeleton | `lane.surface` | `nixfied/lib/mkNixfied.nix`, `nixfied/lib/canonical.nix`, `nixfied/modules/default.nix`, `nixfied/compiler/default.nix`, `nixfied/runner/default.nix`, `nixfied/registry/default.nix` | `flake.nix` | None | `nix flake show` exposes skeleton outputs without eval failure |
| 1 | Module system foundation | `lane.modules` | `nixfied/modules/core.nix`, `nixfied/modules/runtime.nix`, `nixfied/modules/profiles/webapp.nix`, `nixfied/modules/profiles/eth.nix`, `nixfied/project/module.nix` | `nixfied/modules/default.nix` | None | `lib.evalModules` resolves config via imports only |
| 2 | Typed task/workflow options | `lane.modules` | `nixfied/modules/tasks.nix`, `nixfied/modules/workflows.nix` | `nixfied/modules/default.nix`, `nixfied/modules/core.nix` | None | Sample project can define `nixfied.tasks` and `nixfied.workflows` with type validation |
| 3 | Compiler pass scaffolding | `lane.compiler` | `nixfied/compiler/resolve-modules.nix`, `nixfied/compiler/normalize-runtime.nix`, `nixfied/compiler/compile-services.nix`, `nixfied/compiler/compile-tasks.nix`, `nixfied/compiler/compile-workflows.nix`, `nixfied/compiler/compile-views.nix`, `nixfied/compiler/finalize-model.nix` | `nixfied/compiler/default.nix`, `nixfied/lib/mkNixfied.nix` | None | `mkNixfied` returns `model` and `stateHash` |
| 4 | Canonical hash and deterministic ID primitives | `lane.compiler` | `nixfied/compiler/id.nix` | `nixfied/lib/canonical.nix`, `nixfied/compiler/finalize-model.nix`, `nixfied/compiler/compile-tasks.nix` | None | Repeated evaluation yields stable `stateHash` and stable task/workflow IDs |
| 5 | Dispatcher and app/help/docs model views | `lane.runner` + `lane.surface` | `nixfied/runner/dispatcher.nix` | `nixfied/compiler/compile-views.nix`, `nixfied/lib/mkNixfied.nix`, `flake.nix` | None | `apps`, `help`, and `docs` are generated from model only |
| 6 | Universal deterministic workflow executor | `lane.runner` | `nixfied/runner/executor.nix`, `nixfied/runner/env-sandbox.nix` | `nixfied/runner/default.nix`, `nixfied/lib/mkNixfied.nix` | None | `dev/test/build/check/ci` run through one executor with deterministic scheduling |
| 7 | Registry rewrite (strict NDJSON + replay) | `lane.registry` | `nixfied/registry/events.nix`, `nixfied/registry/snapshot.nix`, `nixfied/registry/replay.nix` | `nixfied/registry/default.nix`, `nixfied/runner/executor.nix` | None | Events written as strict NDJSON, replay reproduces terminal state |
| 8 | Operational commands bound to model | `lane.runner` + `lane.compiler` | `nixfied/modules/operations.nix` | `nixfied/compiler/compile-tasks.nix`, `nixfied/compiler/compile-views.nix` | None | `validate-env`, `test-isolation`, `ports`, `check-ports` execute from compiled model data |
| 9 | Introspection + external schema outputs | `lane.surface` | `nixfied/schemas/task-contract-v1.json`, `nixfied/schemas/workflow-contract-v1.json`, `nixfied/schemas/model-export-v1.json` | `flake.nix`, `nixfied/lib/mkNixfied.nix` | None | `.#model`, `.#stateHash`, `.#tasks`, `.#task::<id>`, `.#schema` all work |
| 10 | Installer/wrapper/vendoring flow | `lane.surface` | `nixfied/install/wrapper-flake.nix` | `nixfied/.framework/internal/install.nix` (or replacement installer entrypoint), `flake.nix` | None | `framework::install` defaults to thin wrapper; `--vendor` supported |
| 11 | Determinism and regression gates | `lane.modules` + `lane.compiler` + `lane.runner` + `lane.registry` | `tests/framework/v2/model-hash.nix`, `tests/framework/v2/cross-machine-hash.nix`, `tests/framework/v2/scheduler-order.nix`, `tests/framework/v2/help-snapshot.nix`, `tests/framework/v2/registry-replay.nix` | `tests/framework/README.md` | None | All determinism gates pass locally and in CI |
| 12 | Decommission old architecture | `lane.cleanup` | None | `flake.nix`, `README.md`, `docs/DETAILED.md` | `nixfied/.framework/hooks.nix`, `nixfied/.framework/ci.nix`, `nixfied/.framework/slots.nix`, `nixfied/.framework/internal/core.nix`, `nixfied/.framework/internal/module-apps.nix`, `nixfied/.framework/internal/isolation.nix`, `nixfied/.framework/lib/app-api.nix`, `nixfied/.framework/lib/service-api.nix`, `nixfied/.framework/lib/execution-core.nix`, `nixfied/.framework/lib/process-registry.nix`, `nixfied/project/default.nix`, `nixfied/project/dev.nix`, `nixfied/project/test.nix`, `nixfied/project/prod.nix`, `nixfied/project/quality.nix`, `nixfied/project/format.nix`, `nixfied/project/ci.nix`, `nixfied/project/catalog.nix`, `nixfied/project/lib/command.nix` | No runtime path references removed files; all top-level commands come from new model |

### 20.3 Master checklist (execution order)

- [ ] Phase 0 completed and merged.
- [ ] Phase 1 completed and merged.
- [ ] Phase 2 completed and merged.
- [ ] Phase 3 completed and merged.
- [ ] Phase 4 completed and merged.
- [ ] Phase 5 completed and merged.
- [ ] Phase 6 completed and merged.
- [ ] Phase 7 completed and merged.
- [ ] Phase 8 completed and merged.
- [ ] Phase 9 completed and merged.
- [ ] Phase 10 completed and merged.
- [ ] Phase 11 completed and merged.
- [ ] Phase 12 completed and merged.

### 20.4 PR slicing guidance

To keep review quality high, each phase should be one PR series with:

- one architectural concern per PR,
- deterministic test updates in the same PR,
- explicit before/after command behavior notes in PR description,
- no mixed-phase changes unless required by build breakage.
