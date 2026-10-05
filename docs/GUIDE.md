# Adopting Nixfied

This guide covers the stable adopter workflow: wire an existing project through
`compileManifest` and `projectApps`, author `nixfied.nix`, discover and run the
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
| `flake.nix` | Declare Nixfied and expose the compiled manifest and generated apps |
| `flake.lock` | Pin exact input revisions for reproducible evaluation |
| `nixfied.nix` | Declare project-owned tasks, services, slots, state policy, and exported verbs |

### Existing flake

Add the input, compile the module as the existing `manifest` package, and expose
the existing generated app set:

```nix
inputs.nixfied.url = "github:willyrgf/nixfied";

packages.${system}.manifest =
  nixfied.lib.${system}.compileManifest ./nixfied.nix;

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
nix run .#docs -- api function library/compileManifest
nix run .#docs -- api function library/projectApps
nix run .#docs -- option nixfied.surface.verbs
```

Verb descriptions are Nix-only app metadata and do not enter the manifest.
`projectApps` accepts ordinary Nix modules: paths (including subdirectories),
functions, and attribute sets with composed `imports`. The installer uses
`./nixfied.nix` by convention; help imposes no project layout requirement.

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
returned by `projectApps`. Custom apps remain outside the generated help catalog.

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

`.#help` prints the Nixfied-generated framework controls and exported verbs.
Their definitions supply the descriptions when the catalog is built. Custom
apps merged afterward and later metadata overrides are excluded. Invalid
generated descriptions reject catalog construction. The help program invokes
no Nix commands, executes no manifest, and creates no runtime state or lock file.

Explicit path or remote help references work from any caller directory and show
the referenced project's generated catalog. Per-app `--help` remains the exact
runtime-flag reference.

Read the authoring and API reference at the project's pinned Nixfied revision:

```sh
nix run .#docs
nix run .#docs -- options nixfied.services
nix run .#docs -- option 'nixfied.services.<name>.stateRefs'
nix run .#docs -- topic placeholders
nix run .#docs -- api function library/compileManifest
nix run .#docs -- source
```

`docs --help` shows the full query interface. Option lookup is exact, including
literal `<name>` segments; namespace listing matches whole path segments.
Topic lookup combines selected authored sections with related definitions and
commands for further reference queries. For example, `docs topic runtime` explains runtime
responsibilities, admission, execution, lifecycle, state ownership and cleanup.
Unknown names and empty queries fail with guidance. The realised command reads
static content without a browser, pager, network, manifest admission or runtime
state. It reports the supplying source path and revision when available;
path/dirty sources are not presented as committed revisions. The readable
reference is also in `packages.<system>.docs/share/nixfied/reference/API.md`.

Selecting a project app still evaluates its surrounding flake and exported task
names. If broken authoring prevents that evaluation, invoke `#docs` on the exact
supplying framework source from the project's lock, for example
`nix run /nix/store/<supplying-source>#docs`. Do not substitute an unpinned latest
source. Like generated help, docs does not require the caller's directory to
match the project.

For project-specific manifest facts, build the existing manifest package:

```sh
nix build .#manifest
less result/views/docs.md
```

The generated view lists project identity, ABI and target, allowed slots and
their exact port windows, state policy, task kinds and derived service
requirements, composite steps, services, and endpoints. It is derived from
`result/manifest.json`; neither file should be edited. `manifest.json` remains the
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
- `invocation.timeoutMs` is optional and defaults to `null`: tasks run until
  exit, cancellation, or a started-service failure. A positive value imposes a
  deadline. This also applies to preparation tasks; readiness probing starts
  afterward and retains its own finite attempt limits. Runtime `--timeout-ms`
  limits lifecycle operations, not the selected task's duration.
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
nix build .#manifest
nix run .#manifest-check
```

## Run and control

Use `.#help` for the available project apps and their one-line descriptions.

`manifest-check` is framework admission. A verb such as `check` is project-owned:
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

The default run projection is `summary`: it shows human progress and the live
output of executed tasks, preparation, and services on stderr, one
`[label] line` (or `[label:err] line`) at a time, followed by the result summary
and evidence paths; stdout stays empty. A long-running task such as an HTTP
server shows its output while its database services keep running. A task may
declare `defaultOutput = "task-output"` to make its direct application
interface the default. Explicit `--output <mode>` always wins. `--output json`
writes structured output to stdout with no live logs; `--output both` adds the
structured result on stdout to the live human output. All child output is also
kept in redacted run log files.

Live output is shown by a small helper process that reads those redacted files.
The session never waits for your terminal: if you stop reading, services are
still observed, torn down, and the slot is released; the command then keeps
presenting the remaining output. Ctrl-C ends both the session and its display.

For an application-shaped leaf, request the already redacted captured bytes
explicitly:

```sh
nix run .#run -- --task simulate --output task-output < request.json
```

`task-output` requires one directly selected leaf and writes its exact captured
stdout to stdout and its exact captured stderr among runtime diagnostics on
stderr, live while the task runs. It preserves binary bytes and missing final
newlines on success, task failure, timeout, and cancellation. Check the command status
before treating stdout as a valid result; a child exit accepted by
`exitPolicy.successCodes` still returns success. Requesting `task-output` for a
composite is rejected before runtime state or child side effects. Use `summary`,
`json`, or `both` when you need runtime results rather than the selected leaf's
captured bytes.

If a descendant keeps an output pipe open after child cleanup, capture stops
after a shared one-second shutdown deadline and reports `SECRET_LEAK_BLOCKED`.
The diagnostic names the incomplete stream. Safe prefix log files remain for
inspection (and may already have been shown), but the runtime records the
capture as incomplete and publishes no completed task evidence for that task. This diagnostic does not mean a secret was observed leaking, or that
every escaped descendant was terminated; inspect the program's child-process
behavior along with the retained logs.
There are no mode-specific flag aliases. There is no framework `logs`
command: metadata modes report the evidence paths for inspection.

## Services, slots, and state

Services are generic foreground processes with prepare, start, readiness,
health, stop, and clean semantics. A service may declare one endpoint, several
named endpoints, or no endpoint. Every declared TCP endpoint receives a planned
port and must be proven to belong to the process the runtime started; an open
port alone is not readiness. A conflicting listener rejects startup.
On macOS, the runtime inspects socket FDs held by verified contained processes
and requires complete SDK-sized records with exact socket identities. Inspection
uncertainty that prevents a complete listener witness set reports
`PORT_UNVERIFIABLE`. Preflight checks exact-bind availability before prepare;
managed-listener inspection follows service start.

Each run owns its services until the entire session finishes, then stops them.
Tasks in the same graph share services. A later run starts fresh service
processes; application-data retention is controlled separately by state policy.
Only one session may own a slot at a time; use different slots for concurrent runs.

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

Add `--daemon` to run a task session in the background, for example a
long-running server task and its services:

```sh
nix run .#serve -- --daemon   # prints {"runId", "runDir", "logsDir"}
nix run .#down                # later: cancel that session
```

The printed identity means the session was established, not that the task
succeeded or that services are ready. Its redacted output stays in `logsDir`,
and its final outcome is recorded under that `runId`. `--daemon` cannot be
combined with `--output`, and it rejects tasks that read interactive stdin.

`down` asks the slot's live session to cancel through that session's own
control endpoint and waits for it to finish its normal teardown; if the owning
runtime has died, `down` takes the slot and stops the recorded processes. It
never cancels a newer session that replaced the one it selected.

Each slot has its own state root, registry, process records, and
deterministic candidate port window. `nixfied.placement.ports` controls the base,
window size, and stride; the generated manifest view shows the resolved windows.

Runtime state defaults to `$XDG_STATE_HOME/nixfied`, then the platform-specific
user state directory. Set `NIXFIED_STATE_DIR` to choose another base, for
example in CI:

```sh
NIXFIED_STATE_DIR=/tmp/my-project-state nix run .#check -- --slot 0
```

`clean` is idempotent, path-confined, marker-gated, slot-owned, and
process-gated. Run `down` first. `nixfied.state.persistence` is the only
retention policy: `run-scoped` data is deleted automatically after each
session's processes have stopped (or by the next invocation's recovery if the
runtime died), while `persistent` data survives sessions and additionally
requires `clean --purge`. Purge overrides retention only, never
the ownership, confinement, or live-process checks. An interrupted deletion is
resumed with its original identity before any new data is created. Do not
manually rewrite state markers or the registry.

### Diagnose a state refusal

Before retrying a state or cleanup failure, use the reported error and any
evidence paths to answer these questions:

- Which check failed: access to the state path, marker identity, registry
  integrity, live process ownership, a pending deletion, or persistence? A failure to read
  evidence does not establish that the slot is idle or unowned.
- Are the framework pin, project identity, slot, and state base the ones used
  for the affected run? Keep that context consistent when using `ps`, `down`,
  and `clean`. Use `docs topic context` to check how the state base is selected.
- Does `ps` successfully establish the slot's current state? If inspection
  fails, preserve its diagnostic and existing evidence; do not treat failure
  as an empty process list or delete registry/marker files to proceed.
- Is cleanup blocked only by `persistent` retention, or by ownership,
  confinement, a pending deletion, or processes? `--purge` addresses only
  retention. Use `down` for owned processes; a failed ownership proof needs
  investigation, not a broader deletion command.

Keep the original diagnostics and reported evidence paths when investigating.
For a failure following an upgrade, use `docs topic recovery` to separate
manifest admission from preparation of existing state.

### Descriptive references and state roots

Read the declaration and default with
`nix run .#docs -- option 'nixfied.services.<name>.stateRefs'`.
These labels are not a storage-backend selector or a registry of state roots.
Execution lowering discards these labels, so changing them does not move state
or change runtime service identity. They remain serialized in `manifest.json` and
shown in `views/docs.md`; changing them therefore changes the raw manifest hash.

Service/task `logRefs`, task `artifactRefs`, and task `summaryRefs` are likewise
descriptive manifest/view data, discarded during execution lowering. They do not
choose evidence paths, collect artifacts, or configure cleanup. Runtime evidence
and cleanup retain their native behavior.

Use `${stateDir}` in invocation arguments or environment values for the
runtime-owned slot root. Child programs choose subdirectories beneath it (for
example `${stateDir}/pgdata`); `stateRefs` does not create them. Configure state
retention and cleanup with `nixfied.state`, and use `down`/`clean` for owned
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
from the invocation root; immutable source roots come from the manifest's store source.
Children receive only declared environment variables and the runtime-owned
`PATH` built from invocation tools. Use `docs topic secrets` for secret values.

Host directories are resolved natively, outside `manifest.json`:

| Base | Selection order | Linux HOME fallback | macOS HOME fallback |
| --- | --- | --- | --- |
| Runtime state | explicit `--state-base`, `NIXFIED_STATE_DIR`, `XDG_STATE_HOME/nixfied`, HOME fallback | `.local/state/nixfied` | `Library/Application Support/nixfied` |
| File secrets | `NIXFIED_SECRETS_DIR`, `XDG_CONFIG_HOME/nixfied/secrets`, HOME fallback | `.config/nixfied/secrets` | `Library/Application Support/nixfied/secrets` |

Application data is stored under `<base>/data/<project>/<environment>/<slot>`.
The corresponding `registry/<project>/<environment>/<slot>` directory holds the
registry and `runs/<runId>` evidence. Cleaning application state preserves logs,
artifacts, summaries, and registry history.

Placement changes are an incompatible runtime cutover. Stop or recover old
sessions with their matching runtime before upgrading. Preserve existing data
and history; the new runtime does not migrate or reinterpret them. If the old
runtime cannot safely recover, stop the relevant processes offline and preserve
the old state base before choosing a fresh base. Copying an old marker does not
authorize reuse of preserved data.

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
changes the manifest nor reserves a port; endpoint readiness still requires the
managed listener witness and its attached application probe. A host without that observation produces no
such advisory.


## Secrets

The manifest contains secret descriptors, never values. An environment-backed
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

URL rewrites require literal `inputs.nixfied.url` or equivalent nested attributes.
If rollback leaves recovery files, inspect the reported paths before retrying.
See [UPGRADE-1](CONTRACT.md#output-and-failure-contract) for supported edits and
concurrency limits.

There are no manifest migrations, compatibility shims, or simultaneous old/new
contracts. A new pin compiles a new manifest and ships its exactly matching
runtime. Treat an upgrade as a deliberate contract transition.

For projects adopting the manifest terminology change, update the project-owned
`flake.nix` integration to `compileManifest`, `packages.${system}.manifest`,
and any default-package references before checked candidate evaluation. Update
scripts to `manifest-check` and `manifest.json`; the former names have no aliases. The
registry schema also changes: stop and clean incompatible runtime-owned state
with the previous pin before switching, following the state recovery guidance
below. Existing registry history is never rewritten into the new schema.

Before repinning or editing declarations that could prevent old-pin controls
from evaluating, use the matching old runtime to inspect, stop and recover every
affected slot, or preserve a working old checkout:

```sh
nix run .#ps -- --slot 0
nix run .#down -- --slot 0
```

Application-data compatibility and migration belong to the application and user.
Changing a declaration does not authorize Nixfied to reset retained data. Purge
only when you intend to delete it; existing persistent retention cannot silently
change to run-scoped retention. Then update the input and verify the new manifest:

```sh
nix run github:willyrgf/nixfied#upgrade -- --root . --plan > nixfied-upgrade.diff
nix run github:willyrgf/nixfied#upgrade -- --root .
nix build .#manifest
nix run .#manifest-check
```

To explicitly skip candidate manifest evaluation, choose forced apply:

```sh
nix run github:willyrgf/nixfied#upgrade -- --root . --force
# Repair project wiring and declarations, then build and check the manifest.
```

Use the supplying framework reference and `--root`; adopter-generated apps do
not export `upgrade`. Within the framework checkout, `nix run .#upgrade` supplies
the same command. Read its arguments with `nix run .#docs -- api command upgrade`
from that checkout. `--root` selects the project, and `--nixfied-url URL` requests
an explicit input pin; otherwise the current selection is refreshed. The flake
reference before `#upgrade` selects the tool independently of that candidate.
For the same branch to supply the candidate, also pass its flake reference
through `--nixfied-url`. All locked modes require an identifiable input, the
existing `flake.lock` identifying the old source, and one resolvable temporary
candidate lock. Upgrade owns only the
requested URL assignment and candidate lock; it never edits `nixfied.nix`.

The default installer writes the tracking URL `github:willyrgf/nixfied`;
`flake.lock` holds the exact revision for reproducible builds. Subsequent
upgrades discover new commits with the ordinary command above. A moving branch
URL works the same way, following that branch.

Putting a commit in `inputs.nixfied.url` freezes the selection itself. Refreshing
the lock, including with `--refresh`, preserves that explicitly requested commit.
Upgrade reports the complete original reference and explains a commit-pinned
candidate. To switch a project to tracking, select the repository or branch once:

```sh
nix run github:willyrgf/nixfied#upgrade -- --root . \
  --nixfied-url github:willyrgf/nixfied --plan
# Review, then repeat without --plan to apply the tracking URL and candidate lock.
```

Future upgrades need no `--nixfied-url`; the applied URL selects the channel and
the lock still selects one exact revision. The report's revision-free suggestion
preserves the repository and any named ref; choose a moving branch rather than
a fixed tag when you want future commits. The candidate lock records the selected
URL, and documentation comparison and checked evaluation use its exact locked
source even before that URL is applied to the project.

`--plan` resolves and reports without evaluating `manifest.drvPath` or changing
any project files, even with incompatible project outputs or declarations.
`--plan` wins over `--force` regardless of order or repetition. Default apply
evaluates the candidate manifest derivation and rejects before writes on failure.
Explicit `--force` skips only that evaluation and uses the same guarded write
path. Resolution, ambiguous rewrite, concurrent-file conflict, file-operation
and rollback failures still reject. After a forced repin, repair compile wiring,
declarations and scripts before building and checking the new manifest. Force
does not make old registries or application data acceptable to the new runtime.

Each invocation prints the actual old/candidate source identities (type, original
source, available revision and NAR hash), and a framed unified diff on stdout for
`README.md` and regular files under `docs/`. Status, warnings, next steps and Nix
diagnostics stay on stderr. Redirect stdout as shown above to review a large
diff in a file while keeping the status report visible. Documentation failure
is advisory: unavailable sources have a distinct marker from identical
documentation. The diff is source
evidence, not a complete semantic change inventory or compatibility proof.

The common report says `candidate manifest evaluation: not run (--plan)`,
`passed`, `failed`, or `skipped (--force)`. It shows `would change` for plan,
`blocked` for rejected proposed changes, and `changed` or `unchanged` for apply,
along with declaration preservation and next steps. Plan presents both checked
apply and explicit force. The build/check commands are for after applying and
repairing project wiring or declarations as needed. Each later invocation
resolves upstream again and may select a different candidate. Evaluation success
covers only the values forced by derivation evaluation. Post-upgrade validation
is explicitly not run: build and manifest-check are still recommended.

Success and no-op exit 0; argument errors exit 2; missing/unsupported wiring,
ambiguous URL rewrites or missing old lock exit 3; candidate resolution failure
exits 4; checked evaluation failure exits 5; captured-file conflict exits 6;
apply failure with restored/unchanged files exits 7; rollback failure requiring
inspection exits 8; caught apply interruption exits 130. Exit 5 cannot result
from plan. Before-write failures leave the three captured root files unchanged;
partial-write failures report rollback's actual outcome. Guards cover these root
files, not all imports; per-file atomic exchanges do not promise multi-file
visibility or power-loss durability.

`--no-lock` remains a mechanical URL-only mode: it resolves no candidate lock,
writes no lock, and skips documentation comparison and evaluation with explicit
skip explanations. `--force` has no additional effect there.

If the new manifest is rejected, restore the previous input and lock from version
control, including working old declarations/wiring if needed, and use its
matching runtime for recovery. `manifest-check` checks manifest origin and
shape, ABI and target, closures, secret references, and plan feasibility without
resolving source or secret material or preparing state. First identify whether
the failure occurred in that manifest check or later while preparing or executing
the run; retain the error and any reported evidence paths. Confirm the affected
pin, project identity, slot, and state base before issuing control commands.
Use `docs topic state` for the checks that govern inspection and cleanup.

If the manifest check passes but a run fails while preparing existing state,
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
