# Adopting Nixfied

This guide covers the stable adopter workflow: wire an existing project through
`compileModel` and `projectApps`, author `nixfied.nix`, discover and run the
generated surface, and operate or upgrade it. The complete declaration schema
is in the generated [option reference](OPTIONS.md).

## Install or integrate

### Fresh project

Run the installer from the project root:

```sh
nix run github:willyrgf/nixfied#install -- \
  --project-id my-project \
  --name "My Project"
```

For installer arguments and defaults, use
`nix run github:willyrgf/nixfied#docs -- api command install`.
The installer creates `flake.nix` and `nixfied.nix` only
when they do not already exist. If `flake.nix` exists, it changes nothing and
prints the exact integration fragment to merge manually. It never overwrites an
existing `nixfied.nix`.

The ownership split is intentional:

| File | Responsibility |
| --- | --- |
| `flake.nix` | Declare Nixfied and expose the compiled model and generated apps |
| `flake.lock` | Pin exact input revisions for reproducible evaluation |
| `nixfied.nix` | Declare project-owned tasks, services, slots, state policy, and exported verbs |

### Existing flake

Add the input, compile the module as the existing `model` package, and expose
the existing generated app set:

```nix
inputs.nixfied.url = "github:willyrgf/nixfied";

packages.${system}.model =
  nixfied.lib.${system}.compileModel ./nixfied.nix;

apps.${system} =
  nixfied.lib.${system}.projectApps ./nixfied.nix;
```

Place those expressions inside the system mapping already used by the flake.
After either installation path, create and commit the standard input lock
before evaluating the app surface. A Git worktree must expose new source files
to Nix before lock creation:

```sh
git add flake.nix nixfied.nix  # only when these files are new to Git
nix flake lock
git add flake.lock             # include the generated pin in the commit
```

Outside a Git worktree, only `nix flake lock` is required.

The installer and generated apps intentionally use different framework
products. `.#install` contains only the dependency-free `nixfied-cli`, so
scaffolding does not build or retain the process runtime. Once the project is
locked, runtime-backed project apps reference the release `nixfied-runtime`.
Use `nix run .#docs -- api app project/run` for an app and its command
relationships. The debug runtime and test child remain private to Nixfied's own checks and gates.

If the adopting flake already has a compatible `nixpkgs` input, it may opt into
input convergence explicitly:

```nix
inputs.nixfied = {
  url = "github:willyrgf/nixfied";
  inputs.nixpkgs.follows = "nixpkgs";
};
```

Test this follows relationship against the project's pinned Nixfied revision;
the package expressions still require a compatible nixpkgs package set.

Read the function inputs/results and exported-verb declaration from their
owning definitions:

```sh
nix run .#docs -- api function library/compileModel
nix run .#docs -- api function library/projectApps
nix run .#docs -- option nixfied.surface.verbs
```

Verb descriptions are Nix-only app metadata and do not enter the model.
`projectApps` accepts only the project-local, root-level `./nixfied.nix` form shown above in a flake with
`flake.nix` and `flake.lock`; function, attrset, subdirectory, and externally
sourced modules are unsupported so help can bind its catalog to the defining
source.

Existing custom apps remain ordinary flake apps. Merge disjoint names into the
generated attrset rather than wrapping Nixfied in another interface:

```nix
apps.${system} =
  (nixfied.lib.${system}.projectApps ./nixfied.nix)
  // {
    my-tool = {
      type = "app";
      program = "${myTool}/bin/my-tool";
      meta.description = "Run the project's custom tool";
    };
  };
```

The right-hand side of `//` wins, so keep custom names distinct from every app
returned by `projectApps`. Give every custom app a nonempty `meta.description`
so contextual help can describe the complete surface.

## Discover the project surface

From the adopter project root, use the contextual catalog for app discovery and
the selected app for its exact flags:

```sh
nix run .#help
nix run .#<app> -- --help
```

Catalog entries are reference-neutral names and descriptions. Replace `#help`
in the invocation you used with `#<name>`; local adopter discovery therefore
stays local (`.#help` → `.#check`).

`.#help` evaluates the current system's final flake app set, so it includes
framework apps, exported verbs, and custom apps merged after `projectApps`.
Descriptions come only from each app's `meta.description`; a missing or invalid
description fails the whole catalog instead of producing partial help. This Nix
evaluation neither admits nor executes the model and does not materialise
runtime state, but declaration or app evaluation errors surface directly.

Generated adopter help is bound to its defining source and deliberately uses
the current flake context, so only local `nix run .#help` is supported. An
explicit remote or path adopter reference invoked from elsewhere fails instead
of cataloging the caller. Per-app `--help` remains the exact runtime-flag
reference.

Read the authoring and API reference at the project's pinned Nixfied revision:

```sh
nix run .#docs
nix run .#docs -- options nixfied.services
nix run .#docs -- option 'nixfied.services.<name>.stateRefs'
nix run .#docs -- topic placeholders
nix run .#docs -- api function library/compileModel
nix run .#docs -- source
```

`docs --help` shows the full query interface. Option lookup is exact, including
literal `<name>` segments; namespace listing matches whole path segments.
Topic lookup combines selected authored sections with related definitions and
commands for further reference queries. For example, `docs topic runtime` explains runtime
responsibilities, admission, execution, lifecycle, state ownership and cleanup.
Unknown names and empty queries fail with guidance. The realised command reads
static content without a browser, pager, network, model admission or runtime
state. It reports the supplying source path and revision when available;
path/dirty sources are not presented as committed revisions. The readable
reference is also in `packages.<system>.docs/share/nixfied/reference/API.md`.

Selecting a project app still evaluates its surrounding flake and exported task
names. If broken authoring prevents that evaluation, invoke `#docs` on the exact
supplying framework source from the project's lock, for example
`nix run /nix/store/<supplying-source>#docs`. Do not substitute an unpinned latest
source. Unlike contextual help, docs does not require the caller's directory to
match the project.

For project-specific model facts, build the existing model package:

```sh
nix build .#model
less result/views/docs.md
```

The generated view lists project identity, ABI and target, allowed slots and
their exact port windows, state policy, task kinds and derived service
requirements, composite steps, services, and endpoints. It is derived from
`result/model.json`; neither file should be edited. `model.json` remains the
only semantic input to the runtime.

## Author `nixfied.nix`

A declaration is a Nix module. Use `nix run .#docs -- api argument` to discover
the framework-supplied module arguments. This example combines a packaged API,
the Postgres adapter, leaf tasks, and one exported composite:

```nix
{ pkgs, adapters, ... }:
let
  apiPackage = pkgs.callPackage ./nix/api.nix { };
in
{
  imports = [ adapters.postgres ];

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
    endpoint.endpointId = "api-http";
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
      database = {
        task = "smoke-query";
        dependsOn = [ "lint" ];
      };
      api = {
        task = "api-smoke";
        dependsOn = [ "database" ];
      };
    };
  };

  nixfied.surface.verbs.check = "Run formatting, linting, and tests";
}
```

The main rules are:

- A leaf task owns one inline `invocation`; a composite owns a static DAG of
  named `steps`. Parameters, retries, loops, and conditionals belong in Nix
  expansion or inside a leaf program.
- `invocation.tools` contains declared closure IDs or Nix packages. Their
  executable roots form the child `PATH`. The child otherwise receives only
  declared `env`; the runtime environment is not inherited.
- A leaf's `requires` names services that must be ready while it executes.
  Service `connectsTo` declarations order transitive dependencies and make
  their named endpoints addressable. Composite service requirements are
  derived from their leaves.
- A leaf's `exitPolicy.successCodes` classifies child exit codes as task
  success. The raw child code remains in task evidence, but is not passed through
  as the `nix run` status. An accepted code contributes success to the overall run, while any
  other code produces `TASK_FAILED`.
- Importing an adapter contributes ordinary service and task definitions. It
  starts nothing until a selected leaf requires one of those services.
- `nixfied.surface.verbs` is an explicit public choice and description mapping.
  Imported or internal tasks do not silently become flake apps. The generated
  verb app starts with the description declared in `nixfied.nix`, while final
  flake app metadata remains authoritative after composition and is what
  contextual help renders.
- Tool caches and build directories remain child- and project-owned. Configure
  them through ordinary invocation arguments or `env`; Nixfied does not place,
  lock, report, retain, or clean them.

Read the accepted values and default with
`nix run .#docs -- option 'nixfied.tasks.<name>.exitPolicy.successCodes'`.
For example, a tool whose exit codes `0` and `1` are both successful can declare:

```nix
nixfied.tasks.inspect.exitPolicy.successCodes = [ 0 1 ];
```

If `inspect` exits `1`, Nixfied records that raw code but treats the task as
successful; the overall command therefore returns success unless another task,
lifecycle operation, or runtime phase fails.

Invocations observe the live workspace by default, so invoke the apps from the
intended project root: source resolution starts at the invocation root.
Source policy can instead select an immutable snapshot or flake input. Read
`nix run .#docs -- topic context` for source resolution and child environment
behavior. Inspect the declarations with
`nix run .#docs -- options nixfied.codebases.main`, and use `option` with an exact
path for its type and default. See [OPTIONS.md](OPTIONS.md) for the exact option
types, defaults, lifecycle fields, probe forms, endpoint forms, and validation
vocabulary.

After a declaration change, compile and admit it before running a workflow:

```sh
nix build .#model
nix run .#model-check
```

## Run and control

Use `.#help` for the available project apps and their one-line descriptions.

`model-check` is framework admission. A verb such as `check` is project-owned:
it exists only when the project declares a task with that name and exports it,
and running it executes that project workflow.

`run` starts the selected task's exact derived service closure, then executes
the leaf or flattened composite DAG. Selecting a leaf directly does not run
steps that happen to precede it in some composite; select the composite when
those dependencies are part of the intended workflow.

Pass runtime flags after Nix's `--` separator. Use the app's `--help` for its
flags, or read command arguments, defaults and related records together:

```sh
nix run .#docs -- topic commands
nix run .#docs -- api command run
```

### Choose output

For an interactive run, use `summary` to follow progress and find the evidence
paths. For automation that consumes the runtime's structured result, use
`--output json`; use `--output both` when you also want the human projection.
When a caller needs the selected leaf program's own output, use `task-output`.

The default run projection is `summary`: it writes human progress, the result
summary, and evidence paths to stderr while leaving stdout empty. A task may
declare `defaultOutput = "task-output"` to make its direct application
interface the default. Explicit `--output <mode>` always wins. `--output json`
writes structured output to stdout; `--output both` requests both projections
explicitly. Child stdout/stderr is captured in redacted run log files and is not
replayed inline.

For an application-shaped leaf, request the already redacted captured bytes
explicitly:

```sh
nix run .#run -- --task simulate --output task-output < request.json
```

`task-output` requires one directly selected leaf and writes its exact captured
stdout to stdout and its exact captured stderr among runtime diagnostics on
stderr. It preserves binary bytes and missing final newlines, and replays on
success, task failure, timeout, and cancellation. Check the command status
before treating stdout as a valid result; a child exit accepted by
`exitPolicy.successCodes` still returns success. Requesting `task-output` for a
composite is rejected before runtime state or child side effects. Use `summary`,
`json`, or `both` when you need runtime results rather than the selected leaf's
captured bytes.
There are no mode-specific flag aliases. There is no framework `logs`
command: metadata modes report the evidence paths for inspection.

## Services, slots, and state

Services are generic foreground processes with prepare, start, readiness,
health, stop, and clean semantics. A service may declare one endpoint, several
named endpoints, or no endpoint. Every declared TCP endpoint receives a planned
port and must be proven to belong to the process the runtime started; an open
port alone is not readiness. A live service that is not exactly reusable must
be stopped explicitly with `down` before replacement.

Task service lifetime controls what happens after the borrower finishes:

- `run-scoped` stops the task's services at the end of the run;
- `until-idle` keeps them until reconciliation observes no live borrowers;
- `persistent-until-down` keeps them until an explicit `down`.

Slots isolate simultaneous copies of a project. Declare the accepted range and
default in `nixfied.nix`:

```nix
nixfied.slotPolicy = {
  min = 0;
  default = 0;
  max = 3;
};
```

Select a slot consistently for run and control commands:

```sh
nix run .#check -- --slot 2
nix run .#ps -- --slot 2
nix run .#down -- --slot 2
nix run .#clean -- --slot 2
```

Each slot has its own state root, registry, leases, process records, and
deterministic candidate port window. `nixfied.placement.ports` controls the base,
window size, and stride; the generated model view shows the resolved windows.

Runtime state defaults to `$XDG_STATE_HOME/nixfied`, then the platform-specific
user state directory. Set `NIXFIED_STATE_DIR` to choose another base, for
example in CI:

```sh
NIXFIED_STATE_DIR=/tmp/my-project-state nix run .#check -- --slot 0
```

`clean` is idempotent, path-confined, marker-gated, lease-gated, and
process-gated. Run `down` first. A protected or persistent policy additionally
requires `clean --purge`; purge relaxes only that policy gate, never the
ownership, confinement, or live-process checks. Do not manually rewrite state
markers or the registry.

### Diagnose a state refusal

Before retrying a state or cleanup failure, use the reported error and any
evidence paths to answer these questions:

- Which check failed: access to the state path, marker identity, registry
  integrity, live lease/process ownership, or cleanup policy? A failure to read
  evidence does not establish that the slot is idle or unowned.
- Are the framework pin, project identity, slot, and state base the ones used
  for the affected run? Keep that context consistent when using `ps`, `down`,
  and `clean`. Use `docs topic context` to check how the state base is selected.
- Does `ps` successfully establish the slot's current state? If inspection
  fails, preserve its diagnostic and existing evidence; do not treat failure
  as an empty process list or delete registry/marker files to proceed.
- Is cleanup blocked only by the declared protected/persistent policy, or by
  ownership, confinement, leases, or processes? `--purge` addresses only the
  policy gate. Use `down` for owned processes; a failed ownership proof needs
  investigation, not a broader deletion command.

Keep the original diagnostics and reported evidence paths when investigating.
For a failure following an upgrade, use `docs topic recovery` to separate
model admission from preparation of existing state.

### Descriptive references and state roots

Read the declaration and default with
`nix run .#docs -- option 'nixfied.services.<name>.stateRefs'`.
These labels are not a storage-backend selector or a registry of state roots.
Execution lowering discards these labels, so changing them does not move state
or change service reuse identity. They remain serialized in `model.json` and
shown in `views/docs.md`; changing them therefore changes the raw model hash.

Service/task `logRefs`, task `artifactRefs`, and task `summaryRefs` are likewise
descriptive model/view data, discarded during execution lowering. They do not
choose evidence paths, collect artifacts, or configure cleanup. Runtime evidence
and cleanup retain their native behavior.

Use `${stateDir}` in invocation arguments or environment values for the
runtime-owned slot root. Child programs choose subdirectories beneath it (for
example `${stateDir}/pgdata`); `stateRefs` does not create them. Configure state
compatibility and cleanup with `nixfied.state`, and use `down`/`clean` for owned
state as described above. Child-tool caches remain project-owned.

## Source and invocation context

Use `nix run .#docs -- api argument` for the framework's module argument
inventory, then query an entry such as
`nix run .#docs -- api argument module-argument/system` for its provider and use.
Nixpkgs supplies `lib`, `config`, `options` and `_module` through ordinary module
evaluation; these are native module facilities, not additional framework
providers. Imports, `mkDefault`, `mkForce`, submodule merging and native `apply`
retain their normal meanings. Required values need not be supplied to read the
static framework reference.

Inspect contextual defaults and source identity types in their option entries:

```sh
nix run .#docs -- option nixfied.target.system
nix run .#docs -- option nixfied.codebases.main.sourceIdentity
```

Source identity conversion retains dependency context. Live source roots resolve
from the invocation root; immutable source roots come from the model's store source.
Children receive only declared environment variables and the runtime-owned
`PATH` built from invocation tools. Use `docs topic secrets` for secret values.

Host directories are resolved natively, outside `model.json`:

| Base | Selection order | Linux HOME fallback | macOS HOME fallback |
| --- | --- | --- | --- |
| Runtime state | explicit `--state-base`, `NIXFIED_STATE_DIR`, `XDG_STATE_HOME/nixfied`, HOME fallback | `.local/state/nixfied` | `Library/Application Support/nixfied` |
| File secrets | `NIXFIED_SECRETS_DIR`, `XDG_CONFIG_HOME/nixfied/secrets`, HOME fallback | `.config/nixfied/secrets` | `Library/Application Support/nixfied/secrets` |

The explicit state argument retains the native parser's lexical-path behavior,
including an empty operand; the nonempty rule applies to environment overrides.
When a base is needed, unset HOME without another selected base rejects. Empty HOME is present and
produces the corresponding relative fallback. Directory variables retain OS
path bytes, including non-UTF-8 names; secret material itself must be UTF-8.
A selected secrets directory with missing material fails rather than falling
back to another directory. Environment and file secret values have trailing CR
and LF removed, and empty resulting values reject. Rejected non-UTF-8 environment
secret values never enter diagnostics.

On Linux, runtime admission may warn when a declared candidate-port window
overlaps the observed host ephemeral-port range. This optional advisory neither
changes the model nor reserves a port; endpoint ownership still requires the
native listener/process proof. A host without that observation produces no
such advisory.


## Secrets

The model contains secret descriptors, never values. An environment-backed
secret and its use look like this:

```nix
nixfied.secrets.database-password.source = {
  kind = "env-var";
  envVar = "DATABASE_PASSWORD";
};

nixfied.tasks.migrate.invocation.env.DB_PASSWORD =
  "\${secret:database-password}";
```

A `file` descriptor names a relative path under the configured secrets base
(`NIXFIED_SECRETS_DIR`, or the platform default). On run admission, the runtime
resolves secrets, puts values only in memory and the hermetic child environment,
and redacts runtime-owned persistent output. A file or socket written directly
by a child remains the child's responsibility.

## Use and extend adapters

Adapters are Nix modules that compile a concrete service into the same generic
tasks, services, closures, and lifecycle invocations. Discover the available
adapters and their contributions with `nix run .#docs -- api module`.
For example:

```nix
{ adapters, ... }:
{
  imports = [ adapters.postgres ];
}
```

Override their declarations through normal module merging and reference their
tasks in project composites. Use the [adapter guide](ADAPTERS.md) for prepare
tasks, probes, multi-endpoint services, endpoint-less workers, and adapter
authoring conventions.

## Upgrade and recover

There are no model migrations, compatibility shims, or simultaneous old/new
contracts. A new pin compiles a new model and ships its exactly matching
runtime. Treat an upgrade as a deliberate contract transition.

Before repinning, use the old pin to inspect and stop every active slot:

```sh
nix run .#ps -- --slot 0
nix run .#down -- --slot 0
```

If the new declaration intentionally changes the state epoch, clean incompatible
state with the old pin as well; add `--purge` only for state declared protected
or persistent. Then update the input and verify the new model:

```sh
nix run github:willyrgf/nixfied#upgrade -- --root . --plan > nixfied-upgrade.diff
nix run github:willyrgf/nixfied#upgrade -- --root .
nix build .#model
nix run .#model-check
```

`upgrade` refreshes only the `nixfied` input/lock entry. Read its arguments and
defaults with `nix run .#docs -- api command upgrade`. A checked upgrade requires
the existing `flake.lock`: it uses that exact locked Nixfied source as the old side of the comparison, resolves a candidate lock in
a temporary file, and applies it only after the candidate model passes
preflight. It does not edit `nixfied.nix` or translate old models or state.

The checked command prints a framed unified diff to stdout for `README.md` and
the regular files under `docs/`; status, warnings, and Nix diagnostics go to
stderr. `--plan` performs the same candidate resolution, documentation diff,
and `model.drvPath` preflight without changing project files, so the redirected
diff is safe to inspect before applying. A lock-resolution or model-preflight
failure is nonzero and leaves `flake.nix`, `flake.lock`, and `nixfied.nix`
unchanged. If either source cannot be materialized, the report says that the
documentation diff is unavailable; if the scoped files are identical, it says
that no checked-in documentation changed. Neither result is a compatibility
claim—the model preflight is the gate.

The status report presents each locked source as a readable identity block
with its type, original source, revision when available, and NAR hash when
available. Plan and apply use the same candidate verification, wiring,
ownership, and next-step summary: plan says `would change` and includes the
command to rerun without `--plan`; apply says `changed` when it writes the
candidate. Both report that post-upgrade validation was not run and print the
recommended `nix build <root>#model` and `nix run <root>#model-check` commands.
The documentation diff itself remains the only stdout payload, so this
distinction is preserved when redirecting it to a file.

`--no-lock` is an explicit mechanical URL-only mode. It skips the source
documentation diff and candidate verification and reports both skips; use it
only when that checked inspection is intentionally unavailable.

If the new model is rejected, restore the previous input and lock from version
control and use that pin for recovery. `model-check` checks model origin and
shape, ABI and target, closures, secret references, and plan feasibility without
resolving source or secret material or preparing state. First identify whether
the failure occurred in that model check or later while preparing or executing
the run; retain the error and any reported evidence paths. Confirm the affected
pin, project identity, slot, and state base before issuing control commands.
Use `docs topic state` for the checks that govern inspection and cleanup.

If the model check passes but a run fails while preparing existing state,
an empty temporary state base can help isolate whether the failure depends on
that state. Only retry a task whose effects are appropriate to repeat; this
starts a new run and does not repair the original state:

```sh
probe_state="$(mktemp -d)"
NIXFIED_STATE_DIR="$probe_state" nix run .#run -- --task <id>
```

Do not delete the whole Nixfied state base or bypass marker and registry checks.
When cleanup is necessary, target the affected project slot through `down` and
`clean` using the runtime that owns its contract.

## Keep project documentation project-specific

Nixfied owns the generic option reference, control-command semantics, and the
generated facts in `views/docs.md`. An adopting repository should document only
what its names mean and how its team uses them:

- which exported verb is the normal local or CI entry point;
- when to select focused internal tasks through `.#run`;
- project artifact and child-tool cache policies;
- custom non-Nixfied flake apps and their relationship to workflows;
- CI selection, platform exceptions, and project-specific recovery steps.

Link back to this guide and [OPTIONS.md](OPTIONS.md) for framework behavior
instead of copying it into the adopter repository. This keeps framework updates
in one place while the project's operational vocabulary remains close to its
code.
