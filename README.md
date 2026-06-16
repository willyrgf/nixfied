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

Nix with flakes enabled — the only requirement. The runtime and CLI build from a
pinned Rust toolchain via `nix` (no host Rust needed). `nix develop` provides the
pinned `cargo` / `clippy` / `rustfmt` for development.

## Quickstart

Build a model (a Nix store output; there is no required `manifest.json`):

```sh
nix build .#minimal-model
# → result/{model.json, views/{schema.json,docs.md,capabilities.json}}
```

Drive it with the runtime (`nixfied-runtime`: `check`, `run`, `ps`, `down`,
`clean`):

```sh
rt="$(nix build .#nixfied-runtime --no-link --print-out-paths)/bin/nixfied-runtime"
model="$(nix build .#minimal-model --no-link --print-out-paths)/model.json"

"$rt" check --model "$model"                                             # validate admission contract
NIXFIED_STATE_DIR=/tmp/nixfied "$rt" run   --model "$model" --task smoke # start its services, run it
NIXFIED_STATE_DIR=/tmp/nixfied "$rt" clean --model "$model"              # marker-gated cleanup
```

`run` keeps stdout as pretty JSON for machines, including diagnostic
`durationMs` values and evidence paths (`stdoutPath`, `stderrPath`,
`runSummaryPath`). Human progress, the final pass/fail summary, and those same
paths are written to stderr. Child stdout/stderr is not replayed inline; inspect
the redacted log files through the paths in the JSON or summary.

A model is admitted only from under the Nix store and only on an exact
`runtimeAbi` / `toolchainId` match. Inspect it through the `nixfied` CLI
(`model` / `schema` / `docs` / `capabilities`):

```sh
nixfied="$(nix build .#nixfied-runtime --no-link --print-out-paths)/bin/nixfied"
"$nixfied" capabilities --model "$model"
```

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

## Install into another project

```sh
nix run github:willyrgf/nixfied#install   # scaffolds flake.nix + nixfied.nix (won't overwrite)
nix run github:willyrgf/nixfied#upgrade   # repins the nixfied input only
```

The scaffold wires the generated surface over your model (via
`nixfied.lib.<system>.projectApps ./nixfied.nix`, already in the scaffolded
`flake.nix`). The **control namespace is framework-reserved**; the **project
verbs are yours** — one flake app per task id you export:

```sh
# reserved control namespace
nix run .#run -- --task <id>    # run any declared task (no selection => refuse + list)
nix run .#model-check           # admission: your model is well-formed and admits (no execution)
nix run .#ps                    # observe registry-owned processes (reconciles stale evidence)
nix run .#down                  # stop everything the runtime owns on the slot
nix run .#clean                 # remove the marker-gated slot state
nix run .#clean -- --purge      # also remove protected/persistent state, under the same safety gates

# your verbs (nixfied.surface.verbs = [ "check" "ci" ];)
nix run .#check
nix run .#ci
```

Your **tests are tasks** and your **phases are composites**: name leaves for
each command (its toolchain on PATH, its argv, its env, its service
requirements), compose them into named DAGs, and export the ones that form
your public surface:

```nix
nixfied.tasks.lint.invocation = {
  tools = [ rustToolchain pkgs.git ];          # the leaf's PATH, typed
  run = [ "cargo" "clippy" "--" "-D" "warnings" ];
};
nixfied.tasks.db-test = {
  invocation.tools = [ rustToolchain ];
  invocation.run = [ "cargo" "test" "--features" "db" ];
  invocation.env.DATABASE_URL =
    "postgresql://postgres@\${host:postgres}:\${port:postgres}/postgres";
  requires = [ "postgres" ];                   # ready while the leaf runs
};
nixfied.tasks.check = { kind = "composite"; steps = nixfiedLib.seq [ "lint" "db-test" ]; };
nixfied.surface.verbs = [ "check" ];
```

Running a task brings up exactly the services its leaves require (closed over
wiring and prepare requirements) — there is no environment membership to
curate and nothing for an imported adapter to inject.

Task service lifetime defaults to `run-scoped`. Set
`serviceLifetime = "until-idle"` to leave the required service closure up until
the next runtime invocation observes no live borrowers, or
`serviceLifetime = "persistent-until-down"` to leave it standing until
`nix run .#down`.
Later compatible runs borrow the existing instance on the exact service identity
instead of starting a second copy.

Source is declared as a `codebase`. The default `live-workspace` mode observes
the caller's checkout and allows `dirtyPolicy = allow|warn`; immutable
`snapshot` / `flake-input` modes carry a Nix store root in `sourceIdentity` and
may use `dirtyPolicy = reject`.

Services and tasks address each other **by name, never by port arithmetic**.
Invocation args and env values support `${port}`/`${host}` (own primary
endpoint for a service, primary requirement for a task), `${stateDir}` (the
slot state root), and the named forms
`${port:<serviceId>}`/`${host:<serviceId>}` for any service declared in
`connectsTo` (services) or `requires` (tasks):

```nix
nixfied.services.app = {
  connectsTo = [ "postgres" ];   # starts postgres first, makes it addressable
  lifecycle.start.invocation = {
    tools = [ "app" ];
    run = [ "my-app" "--db" "\${host:postgres}:\${port:postgres}" ];
    env.DATABASE_URL = "postgres://\${host:postgres}:\${port:postgres}/db";
  };
};
```

Secrets are declared as descriptors (`env-var` or confined `file`), never model
values. Use `${secret:<id>}` only in invocation environment values; the runtime
resolves secrets before spawning, injects them into the hermetic child env, and
redacts runtime-owned persistent output.

Every slot gets a disjoint, deterministic port window, so slot 1's `app` always
talks to slot 1's `postgres`. An undeclared named reference, an undeclared
`connectsTo` target, or a wiring cycle is a compile/admission error, never a
runtime surprise. Keep `nixfied.placement.ports.base` outside the OS ephemeral
port range (Linux 32768-60999; the default 23080 already is) — the runtime
warns at admission if a slot window overlaps it.

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
*own* Rust source with plain cargo/nix — a nixfied task cannot invoke Nix, and
the runtime must not grade itself — while every adopter's verification *is*
composition: the tasks they name and export. See
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md); the exact dev commands are in
[`AGENTS.md`](AGENTS.md).
