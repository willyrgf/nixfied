# RFC: show source documentation diffs during upgrade

Status: implemented

## Summary

`nix run github:willyrgf/nixfied#upgrade` currently repins the `nixfied` flake
input and refreshes its lock entry. It deliberately preserves the adopter's
`nixfied.nix`, because that file contains project-owned semantics. That
ownership boundary is correct, but the command currently gives the adopter
almost no explanation of what changed inside the newly selected framework
source.

This RFC proposes that `upgrade` show the actual diff of Nixfied's checked-in
framework documentation between the adopter's exact old locked source and the
exact candidate source. The diff is derived from the source snapshots involved
in this upgrade; it is not an authored migration-note catalog.

The diff is advisory context. Candidate model evaluation remains the
authoritative compatibility check. `upgrade` never rewrites `nixfied.nix`,
interprets the diff as a migration, or changes the model/runtime ABI.

## Problem situation

An adopter reported this transition:

- old Nixfied revision: `8e1fa56`;
- new Nixfied revision: `0aaf874`;
- relevant change: `7eaeeca`, `describe exported task verbs`.

Before the change, the project declaration was:

```nix
nixfied.surface.verbs = [ "check" "simulate" "test" ];
```

The list contained only task IDs. The generated app descriptions were owned
by Nixfied.

After the change, the exact declaration became a mapping whose values are
project-owned descriptions:

```nix
nixfied.surface.verbs = {
  check = "Run formatting, linting, and strict static analysis";
  simulate = "Run the deterministic noninteractive V1 simulation";
  test = "Run the deterministic core test suite";
};
```

The new source also validates that exported names refer to declared tasks,
avoid framework app names, and have nonempty descriptions that evaluate
successfully.

The old declaration was intentionally not rewritten by `upgrade`. The
adopter therefore saw a small `flake.lock` change, followed later by a Nix
evaluation error during `nix build .#model`. The lock diff was technically
correct but hid the fact that the locked source contained a declaration-surface
change. The adopter had to discover the relevant source commit and compare the
documentation independently.

The current implementation in [`nix/install/upgrade.nix`](nix/install/upgrade.nix)
also suppresses `nix flake update`'s source-change output and reports only the
files it changed or preserved. The result explains ownership, but not the
behavioral transition inside the new input.

## Goals

1. Show the adopter the actual checked-in documentation diff between the old
   and candidate Nixfied sources.
2. Derive that diff from the exact lock identities participating in this
   upgrade, rather than from a manually maintained notes file.
3. Make the diff available directly in the command output and cleanly
   redirectable to a file.
4. Preserve the project ownership boundary: `nixfied.nix` is never rewritten.
5. Keep model evaluation as the compatibility gate and fail closed before
   applying an incompatible candidate.
6. Handle GitHub, Git, path, tarball, and custom lock identities without
   fabricating a revision that Nix did not resolve.
7. Keep the feature Nix-only and outside `model.json`, `runtimeAbi`, and the
   Rust runtime.

## Non-goals

- Do not create or maintain authored migration notes, a release-note schema,
  or a per-version compatibility catalog.
- Do not infer semantic migrations by parsing Markdown or diff hunks.
- Do not translate the old declaration into the new declaration.
- Do not synthesize task descriptions. Those descriptions are adopter-owned
  semantics.
- Do not diff generated adopter output such as `views/docs.md`; it is a
  disposable model projection, not source documentation or authority.
- Do not make GitHub history, a Git remote, or a network API a required source
  of upgrade correctness.
- Do not add a runtime command, model field, ABI capability, state migration,
  or compatibility fallback.

## Proposed user experience

The normal upgrade command produces a source-bound report and the actual diff.
The status framing can look like this:

```text
Nixfied upgrade candidate
  old source:       github:willyrgf/nixfied @ 8e1fa56
  candidate source: github:willyrgf/nixfied @ 0aaf874
  project file:     nixfied.nix (preserved; project-owned)

--- BEGIN NIXFIED DOCUMENTATION DIFF ---
diff --git a/docs/GUIDE.md b/docs/GUIDE.md
...
- nixfied.surface.verbs = [ "check" ];
+ nixfied.surface.verbs.check = "Run formatting, linting, and tests";
...
diff --git a/docs/OPTIONS.md b/docs/OPTIONS.md
...
--- END NIXFIED DOCUMENTATION DIFF ---

model preflight: failed
upgrade not applied; no project files were changed
```

The output is an actual unified diff from the two materialized source
snapshots. Nixfied does not add an interpretation paragraph that could drift
from the source. The surrounding identity and status lines explain what the
diff represents.

The diff is emitted on stdout and status, warnings, and Nix diagnostics are
emitted on stderr. An adopter can therefore save the exact diff without
capturing unrelated status output:

```sh
nix run github:willyrgf/nixfied#upgrade -- --root . > nixfied-upgrade.diff
```

The first implementation should show the complete diff within the explicit
framework-document scope. A later `--no-docs-diff` or quiet mode may be added
for automation if needed; truncating the default diff without saying so would
defeat the purpose of this feature.

`--plan` should run the same candidate resolution, source diff, and model
preflight without applying `flake.nix` or `flake.lock`. This gives adopters a
safe way to inspect the actual change before changing the project.

## Documentation scope

The diff scope is deliberately explicit and stable:

- `README.md`;
- every file under `docs/`.

This includes `docs/CONTRACT.md`, `docs/GUIDE.md`, and the checked-in generated
option reference `docs/OPTIONS.md`, which are the files most likely to explain
an authoring-surface change. It excludes source code, examples, `.git`
metadata, and generated `views/docs.md` output.

The file set is compared in canonical byte-wise order. Files present in only
one snapshot are shown as additions or deletions. If there are no changes in
the scope, print an explicit empty diff marker. That means “no checked-in
documentation changed”, not “the source has no behavioral changes”.

## Source identity and materialization

The old source is the `nixfied` node selected by the adopter's existing
`flake.lock`. The candidate source is the `nixfied` node in a temporary lock
produced by the requested upgrade. The command must not compare the old lock
with an assumed `HEAD`, nor assume that the source running `.#upgrade` is the
source that the adopter's flake will finally resolve.

The full locked identity is retained for reporting:

- GitHub/Git sources: URL and resolved `rev`, with `narHash` when present;
- path sources: resolved path and hash identity when present;
- tarballs: URL and content hash;
- other Nix-supported sources: the locked fields Nix provides.

The implementation should ask Nix to materialize the source nodes using the
old and candidate lock files, rather than reconstructing each source type in
shell. `nix flake archive --json` with the appropriate reference lock is the
intended primitive: it returns store paths for the resolved flake sources. A
small JSON query extracts the `nixfied` source path from each result. The
temporary source paths are copied or normalized under temporary `old/` and
`new/` labels before invoking `diff`, so store paths and host details do not
appear in the human diff.

Documentation materialization is best effort. If the old or candidate source
cannot be reconstructed—for example, a local path was removed or a private
source is unavailable—report:

```text
documentation diff unavailable: <reason>
```

Never report “no changes” in that case. Diff retrieval failure must not be
treated as proof of compatibility and must not be the reason an otherwise
valid upgrade fails. Candidate model evaluation remains the gate.

## Candidate resolution and apply order

The current script rewrites `flake.nix` before refreshing the lock. That order
can leave a rewritten URL behind if lock resolution fails. The RFC proposes a
small transactional refactor around the existing behavior:

1. Record hashes or equivalent identities for `flake.nix`, `flake.lock`, and
   `nixfied.nix` before doing work.
2. Resolve the candidate lock into a temporary lock path using Nix's
   `--output-lock-file`. When `--nixfied-url` is supplied, use Nix's input
   override while producing the candidate lock; do not rewrite the project
   file yet.
3. Materialize the old and candidate source snapshots and emit the docs diff.
4. Evaluate the candidate project model against the temporary lock. The
   minimal check is evaluation of the project's `model.drvPath`, which forces
   module resolution and validation without realizing every declared closure.
5. If candidate evaluation fails, preserve Nix's diagnostic, report that the
   candidate was not applied, and leave all project files byte-for-byte
   unchanged.
6. Recheck the recorded project-file identities so a concurrent edit cannot be
   overwritten.
7. Apply the staged `flake.nix` URL rewrite, when requested, and move the
   candidate lock into place. Report the old and new identities and that
   `nixfied.nix` was preserved.

`--no-lock` intentionally remains a mechanical mode. Without a candidate lock,
the command cannot claim to have compared or validated the new source. It must
say that documentation diff and candidate verification were skipped. A
requested lock refresh that fails must return nonzero and must never be
reported as a successful upgrade.

## Output and exit behavior

The existing usage and refusal errors remain unchanged where possible. The
new cases are:

- candidate lock resolution failure: nonzero, no project mutation;
- candidate model evaluation failure: nonzero, no project mutation;
- documentation materialization failure: warning only, unless model
  preflight or lock resolution independently fails;
- `--plan`: successful inspection never mutates project files;
- successful apply: report the exact source transition, documentation result,
  preserved `nixfied.nix`, and the next commands:

  ```text
  nix build <root>#model
  nix run <root>#model-check
  ```

The command must not imply that a successful docs diff means the model is
compatible. Conversely, a model failure should identify the target source and
the unchanged project declaration, while leaving the original Nix error
visible for precise diagnosis.

## Why this is the smallest design

This proposal composes primitives Nixfied already relies on:

- the existing lock file is the source of old identity;
- Nix resolves and materializes the candidate source;
- `diff` compares ordinary source files;
- Nix evaluates the project model;
- existing ownership rules decide which files may be applied.

It does not introduce a second semantic description of framework changes. The
documentation remains owned by the source documentation files, and the model
remains owned by Nix evaluation. The upgrade command only presents the source
transition the adopter has requested.

A GitHub compare URL alone is insufficient because it is not the actual diff,
does not work for path/tarball/custom sources, and requires the adopter to
leave the command. A Git-history fetcher or permanent upgrade-report artifact
would add network, Git, retention, and formatting responsibilities. The old
architecture had broader remote-fetching upgrade-report machinery; this RFC
does not revive it.

Automatic rewriting is also rejected. The docs diff can show the exact new
shape, but it cannot safely choose project-specific descriptions or preserve
the intent of arbitrary Nix module composition.

## Verification plan

The implementation belongs in the Nix-layer adoption gate in
[`nix/gate-nix.nix`](nix/gate-nix.nix), not in Rust tests.

The implemented gate uses the checked-in fixtures under
[`nix/fixtures/upgrade-golden/`](nix/fixtures/upgrade-golden/) and proves:

1. The exact historical source revisions `8e1fa56` and `0aaf874` are frozen as
   deterministic tarballs, with archive SHA256, NAR hash, normalization, and
   provenance recorded in `manifest.json`.
2. The `surface.verbs` list-to-attrset change appears in the emitted diff and
   candidate model evaluation rejects the old declaration produced by the
   historical installer.
3. A failed candidate leaves `flake.nix`, `flake.lock`, and `nixfied.nix`
   byte-for-byte unchanged.
4. A successful candidate applies only the intended input URL/lock changes and
   preserves `nixfied.nix`.
5. `--plan` and apply each emit the same byte-for-byte `expected.diff`; plan
   performs no mutation, while apply is followed by a model build and
   `model-check`.
6. The exact empty-diff marker is emitted when the locked source is unchanged.
7. Git, path, and tarball locks report their own identities without fabricated
   revisions; an unavailable path source reports explicit diff unavailability
   and still allows an independently valid candidate preflight. An unsupported
   locked scheme is rejected during candidate resolution with no partial report
   or project mutation.
8. The diff is on stdout and status/diagnostics are on stderr, so redirection
   captures the actual diff.
9. No runtime command, runtime state directory, Rust model admission, or ABI
   snapshot is involved in producing the report; Nix-side model preflight is
   intentionally part of the upgrade gate.

The existing negative evaluation proving that the former list form is rejected
must remain. It proves the compiler contract; the new adoption fixture proves
that `upgrade` exposes the relevant source change before the adopter applies
it.

## Contract impact

This is a Nix-only upgrade CLI and output change. It does not modify:

- `model.json`;
- `views/docs.md`;
- the Rust model or runtime;
- `runtimeAbi` or `capability.txt`;
- runtime commands, state, lifecycle, or recovery semantics.

The upgrade section of `docs/GUIDE.md` documents the user-facing output and
apply guarantees. This fixture change adds no new public behavior, so it does
not require a `docs/CONTRACT.md` or ABI change. No runtime compatibility path
is required.

## Open questions

- Whether the first implementation should add an explicit `--no-docs-diff`
  suppression flag for automation, or wait until a real output-volume problem
  appears.
- Whether the JSON extraction for `nix flake archive --json` should use a
  minimal `jq` runtime dependency or a Nix expression that emits only the
  selected source path.
- Whether source materialization should archive the complete project input
  graph or gain a narrower Nixfied-only source query after the first gate
  implementation demonstrates the cost.
