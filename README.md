# Nixfied

Describe your project's services and tasks — leaves, composites, and the
verbs you export — once in typed Nix; a generic, Nix-free Rust runtime then
starts, inspects, reconciles, and cleans them up — many isolated slots side by
side, every process and port under one owner.

## The problem

**A project's operational behavior has no owner.**

A modern project is a small polyglot system — APIs, workers, databases, queues,
migrations, test harnesses, CI jobs, dev workflows. Its *operational* behavior is
scattered across `flake.nix`, shell scripts, package scripts, CI YAML, compose
files, env files, and port conventions, and nothing owns **running** it: starting
its services, running its tasks, tracking processes, owning ports and state, and
cleaning up — let alone running several copies side by side. So `dev`/`test`/`ci`
runs collide, ports and state leak, a service started by one script is stopped by
another (if at all), and CI leaves weak evidence of what ran or survived.

Nixfied gives that system one authority. Typed Nix is the authority for what the
project is *allowed to be*; a generic Rust runtime is the authority for what it is
*currently doing* — so services, tasks, state, ports, logs, and cleanup all
answer to a single owner, per slot.

The model speaks **your** vocabulary as names over a small closed algebra of
two kinds: a **task** is a leaf (a toolchain + argv + env + wiring) or a
composite (a static DAG of named steps — your `check`, your `ci`); a
**service** is durable execution (orchestrated, probed, owned, cleaned),
listening or not. Everything derivable is derived — the services a task
needs, operation bindings, operation ids — and the flake verbs your project
exposes come from the task names *you* export.

- **Nix** is the integration + correctness layer — you describe services,
  tasks, composites, state, ports, and source policy as typed Nix.
- **Rust** is the hidden, generic runtime — it knows no domain and never invokes Nix.
- **`model.json`** is the only semantic seam; `schema` / `docs` / `capabilities`
  are disposable views over it.

The capability line is implemented and shipped: services (endpoint-less ones
included), leaf and composite tasks, derived service unions, multi-slot
isolation, the Postgres and Reth reference adapters, a toolchain-shaped
example, non-destructive `install` / `upgrade`, and a self-hosted gate.

For the design rationale (the *why*) see [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md);
for the contributor contract and invariants see [`AGENTS.md`](AGENTS.md).

## Prerequisites

Nix with flakes enabled — the only requirement.

## Quick Start: install into your project

Install Nixfied into a project by adding its generated flake surface. You keep
your operational declarations in `nixfied.nix`; Nix compiles them into a
Nix-store `model.json`; the generated apps run the matching Rust runtime against
that model. The runtime itself never invokes Nix.

### 1. Install the project wiring

From the root of the project you want to install Nixfied into, when it does not
already have a `flake.nix`:

```sh
nix run github:willyrgf/nixfied#install -- \
  --project-id my-project \
  --name "My Project"
```

This installs two files:

| File | Owner | Purpose |
| --- | --- | --- |
| `flake.nix` | Nixfied wiring | pins the `nixfied` input, exposes generated apps, and keeps the model build behind them |
| `nixfied.nix` | project | declares your services, tasks, composites, slots, source policy, and exported verbs |

If `flake.nix` already exists, the installer refuses to modify it and prints the
exact input/package/app snippets to merge by hand. It never overwrites an
existing `nixfied.nix`.

### 2. Run the installed starter

The installed `nixfied.nix` imports `adapters.synthetic`, which contributes a
tiny TCP service and a `smoke` task. The starter exports that task as a project
verb, so the project works before you replace it:

```sh
nix run .#smoke                   # exported task verb
nix run .#run -- --task smoke     # reserved control app for any declared task
nix run .#ps                      # reconciled process view for the current slot
nix run .#down                    # stop runtime-owned processes for the slot
nix run .#clean                   # marker-gated cleanup for the slot
```

`run` defaults to the human projection: progress, the final pass/fail summary,
and evidence paths (`run-summary`, `logs`) on stderr, with stdout empty.
Automation opts in with `--json`; diagnostics can request both with `--both`.
Child stdout/stderr stays in redacted log files and is not replayed inline.

### 3. Replace the starter declarations

Edit `nixfied.nix`. Keep the project/source metadata, remove the synthetic
adapter when you no longer need it, and declare your own graph:

```nix
{ pkgs, adapters, nixfiedLib, ... }:
let
  apiPackage = pkgs.callPackage ./nix/api.nix { };
in
{
  imports = [ adapters.postgres ]; # imports definitions only; it starts nothing

  nixfied.project.projectId = "my-project";
  nixfied.project.name = "My Project";
  nixfied.codebases.main.logicalRoot = ".";

  nixfied.closures.api = {
    package = apiPackage;
    executable = "bin/api-server";
    effects = [ "process" "network-listener" ];
  };

  nixfied.services.api = {
    connectsTo = [ "postgres" ];
    lifecycle.start.invocation = {
      tools = [ "api" ];
      run = [
        "api-server"
        "--listen"
        "127.0.0.1:\${port}"
        "--database-url"
        "postgresql://postgres@\${host:postgres}:\${port:postgres}/postgres"
      ];
    };
    endpoint = {
      endpointId = "api-http";
    };
  };

  nixfied.tasks.lint.invocation = {
    tools = [ pkgs.bash pkgs.git ];
    run = [ "bash" "-c" "git diff --check" ];
  };

  nixfied.tasks.api-smoke = {
    invocation = {
      tools = [ pkgs.curl ];
      run = [ "curl" "-fsS" "http://\${host}:\${port}/health" ];
    };
    requires = [ "api" ];
  };

  nixfied.tasks.check = {
    kind = "composite";
    steps = {
      lint.task = "lint";
      db = {
        task = "smoke-query"; # contributed by adapters.postgres
        dependsOn = [ "lint" ];
      };
      api = {
        task = "api-smoke";
        dependsOn = [ "db" ];
      };
    };
  };

  nixfied.surface.verbs = [ "check" ];
}
```

The important authoring rules are:

- tasks are leaves (`invocation`) or static composites (`steps`);
- services start only because a task leaf `requires` them, closed over
  `connectsTo` and prepare requirements;
- imported adapters contribute ordinary definitions, not environment
  membership or implicit startup;
- child environments are hermetic: declared `env` plus the runtime-owned PATH
  assembled from `invocation.tools`;
- tool acceleration remains child/tool/project-owned and is expressed through
  ordinary invocation `env` or arguments when needed;
- services and tasks address dependencies by declared names (`${host:postgres}`,
  `${port:postgres}`), not port arithmetic;
- `nixfied.surface.verbs` is the project-owned public surface. The control names
  `run`, `ps`, `down`, `clean`, and `model-check` are reserved.

After each edit, run:

```sh
nix run .#check
```

Use `nix run .#model-check` when you want admission feedback without starting
services or executing tasks.

### 4. Operate and upgrade

The generated apps accept runtime flags after `--`, including `--slot`,
`--timeout-ms`, `--json`, and `--both`:

```sh
NIXFIED_STATE_DIR=/tmp/my-project nix run .#check -- --slot 1 --json
nix run .#down -- --slot 1
nix run .#clean -- --slot 1
```

The scaffold supports slot `0` by default. To run several copies of the same
project side by side, declare the slot range in `nixfied.nix`:

```nix
nixfied.slotPolicy = {
  min = 0;
  default = 0;
  max = 3;
};
```

Each slot gets a separate state root, registry, leases, process records, and
deterministic port window. With the default placement policy, adjacent slots are
offset by `nixfied.placement.ports.slotStride = 100`.

The compiler rejects any selectable task whose derived service endpoint demand
cannot fit its port window. At runtime, endpoint-keyed OS locks coordinate
independent state roots before service-specific mutation; Linux uses
`SOCK_DIAG` and macOS uses the TCP PCB list to prove listeners. A wildcard
listener conflicts with an exact endpoint but cannot satisfy exact readiness.
Proven lock/listener collisions are `PORT_CONFLICT`; a service that simply never
opens its exact endpoint reaches `READINESS_TIMEOUT`.

Task service lifetime defaults to `run-scoped`. Set
`serviceLifetime = "until-idle"` to keep a task's service closure up until a
later runtime invocation observes no live borrowers, or
`serviceLifetime = "persistent-until-down"` to keep it up until `down`.

Secrets are descriptors only (`env-var` or confined `file`); the model never
carries secret values. Use `${secret:<id>}` only in invocation `env` values.
The runtime resolves secrets at admission, injects them into the hermetic child
environment, and redacts runtime-owned persistent output.

Tool build artifacts are ordinary project state, not Nixfied resources. For
example, a project may choose one worktree-owned Cargo target for broad checks:

```nix
nixfied.tasks.check.invocation = {
  tools = [ pkgs.cargo ];
  env.CARGO_TARGET_DIR = "target/verification";
  env.CARGO_INCREMENTAL = "0";
  run = [ "cargo" "clippy" "--workspace" ];
};
```

Nixfied passes that declaration unchanged. Cargo and the project own placement,
writer locking, fingerprints, inspection, retention, cleanup, bypass, and
corruption recovery. `NIXFIED_STATE_DIR` neither selects nor selectively cleans
such artifacts; runtime output contains execution evidence, not cache evidence.

To repin Nixfied later:

```sh
nix run github:willyrgf/nixfied#upgrade -- --root .
```

`upgrade` updates the Nixfied input/lock only. It does not edit `nixfied.nix`,
migrate old models, or provide cross-version compatibility; rebuild the model
after upgrading.

## Examples

| Example | Shows |
| --- | --- |
| `examples/minimal` | the smallest service + task |
| `examples/postgres` | the Postgres reference adapter (idempotent initdb → server → `pg_isready` probes → `SELECT 1` → clean) |
| `examples/reth` | the Reth reference adapter (dev node → JSON-RPC probes → smoke call → clean) |
| `examples/composite` | a composite task (a bounded step DAG over a service) |
| `examples/polyglot-stack` | two services in different languages, one composed check |
| `examples/downstream` | a realistic small system + the copy-paste adoption guide |
| `examples/toolchain` | the toolchain-shaped adopter: multi-tool PATH leaves, heterogeneous requirements, nested composites, and an endpoint-less worker |

Build any via the root flake (e.g. `nix build .#postgres-model`);
`examples/downstream/README.md` walks through adoption.

## Verify (working on Nixfied itself)

One command runs the whole repo, fail-fast — the source gate, the test floor,
then the gate:

```sh
nix run .#ci
```

Its stages also run on their own:

```sh
nix run .#check   # nix flake check (rustfmt + clippy -D warnings + every build) + model admission
nix run .#test    # the white-box cargo floor (binds ports / spawns process groups)
nix run .#gate    # the framework gate (below)
```

`.#gate` exercises the runtime the way adopters do — it runs the example models as
ordinary top-level runs through the runtime under test (each example is its own
spec) — plus the checks a single run can't make: cross-slot isolation, fail-closed,
and a real `install` + `upgrade`. CI (`.github/workflows/checks.yml`) runs the same
layered gate.

Note the asymmetry with the adopter surface above: the framework verifies its
*own* Rust source with plain cargo/nix because those checks exercise the Nix
compiler, install tooling, and Rust implementation, while every adopter's
verification *is* composition: the tasks they name and export. See
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md); the exact dev commands are in
[`AGENTS.md`](AGENTS.md).
