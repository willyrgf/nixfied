Scope: validated against MFM's actual vendored checkout at `/Users/willyrgf/dev/rust/src/github.com/willyrgf/2/mfm/nixfied`, then compared against current upstream.

Real Problems Observed In MFM Usage

- MFM still carries a monolithic project layer. Their `nixfied/project/module.nix` is 3165 lines, and `nixfied/project/default.nix` only imports that file. This makes rebases, review, and downstream migration harder than they need to be. Upstream's split `nixfied/project/{runtime,services,tasks,workflows}.nix` is a real improvement, not cosmetic cleanup.

- Shared runtime roots are an intentional downstream behavior in MFM, not accidental drift. In `nixfied/project/conf.nix`, MFM still uses shared roots:
  - `directories.base = ${XDG_DATA_HOME:-$HOME/.local/share}/${project.id}`
  - `process.registryRoot = /tmp/nixfied-runtime/${project.id}`
  - `process.artifactsRoot = /tmp/ci-artifacts/${project.id}`
  Their portfolio snapshot flow also derives persistent service scope roots under `~/.local/share/${project.id}/<env>/slot-<slot>` when reuse/discovery are persistent or global. Upstream now optimizes for workspace-scoped isolation, but MFM clearly wants an explicit supported shared-root mode.

- The portfolio snapshot app is compensating for current workflow-model limits. `task.mfm.portfolio.snapshot` has to:
  - reserve stdout for the final JSON payload
  - route logs to `/dev/tty` when available
  - create a temporary session dir and handoff file
  - invoke hidden setup and teardown tasks
  - validate the final JSON envelope before replaying it to stdout
  This is not just ad hoc shell. It is a workaround for a real limitation: one public workflow surface cannot currently express per-phase args, handoff state, service session ownership, and a strict stdout contract cleanly.

- MFM hand-built a service/session layer inside project code. `task.mfm.portfolio.services-start` computes owner scope, discovery scope, reuse policy, cleanup policy, runtime-root selection, and handoff JSON itself. That is a real sign that downstreams need a higher-level session/bootstrap API.

- MFM also hand-built a multi-service CI parity bootstrap in `task.ci.services-start`. That task manually:
  - checks required hooks
  - starts postgres, minio, reth, and optional helios
  - polls readiness
  - ensures the Minio bucket exists
  - captures logs for failure reporting
  - handles partial failure cleanup
  This is a strong signal that a higher-level "service set bootstrap" helper would remove a lot of repeated project shell.

- The `nixfied/local/default.nix` seam is genuinely confusing in practice. MFM defines real user-owned local apps there (`aave-v3-origin-fetch`, `aave-v3-origin-compile`, `aave-v3-origin-deploy`), but the root `flake.nix` still passes `localOverrides = [ ]`. So the extension seam exists on disk but is not wired into the exported flake by default. The framework should not silently auto-load locals, but it should make this opt-in seam much harder to misread.

- MFM added custom convenience introspection in `nixfied/project/model-introspection.nix` plus CI assertions in `task.ci.shell-app-contracts`. The real problem is narrower than "framework has no introspection". The vendored framework already exposes the raw information MFM needs:
  - `model.views.apps`
  - compiled `tasks`
  - compiled `workflows`
  - compiled `workflow.plan`
  - launcher metadata derived from `selectionIndex`
  MFM still wrote a custom helper because there is no small, documented convenience surface that directly answers "what app names, task ids, workflow ids, and plan task ids exist?"

- The introspection problem is worse than "inconvenient". For a user trying to understand a simple command like `nix run .#check`, the framework does not provide a direct machine-readable explanation of:
  - which task or workflow that app resolves to
  - which launcher path is involved
  - which runtime surface is materialized
  - why unrelated tasks or packages appear in the build closure
  The answer exists only indirectly across framework internals. In practice that means even an agent cannot simply query an introspection surface and answer the question. It has to start debugging the launch path and infer the result from implementation details.

- A concrete example of the opacity today: in MFM it is not obvious why `nix run .#check` can rebuild `mfm_cli`, even though `check` does not semantically need the CLI binary. The reason is architectural, not task-local:
  - `.#check` goes through the selected-app launcher path
  - that resolves to the shared orchestrator / executor runtime
  - the executor writes `nixfied-model.json` from the full compiled model
  - the compiled task graph stores `runtimeInputs` as `builtins.toString` values
  - unrelated tasks like `task.mfm_cli` and `task.mfm.portfolio.snapshot.exec` include `conf.packages."mfm-cli"` in their `runtimeInputs`
  - `conf.packages."mfm-cli"` is a real `buildRustPackage` over the repo
  So a simple command like `.#check` can end up realizing unrelated package builds through the full-model closure. That is a real introspection and debuggability problem, not just a documentation gap.

- MFM's existing discovery surfaces do not solve this. Their custom `project/model-introspection.nix` only exports task ids, workflow ids, CI modes, and workflow plan task ids. Their generated `docs/repo-index.json` is also too shallow to explain real command behavior. Neither surface explains command resolution, launcher stages, selected runtime closure, or why a given app is causing a particular derivation to build.

- Some older critiques are now stale against MFM's current checkout. In particular, `nixfied/VENDORED.txt` already records a concrete framework revision and compare link, so vendored metadata is not a live problem here anymore.

What This Means

- The strongest valid critique is not "MFM adapted a lot, therefore the framework is missing everything."
- The stronger critique is: the framework already has useful low-level pieces, but downstreams like MFM still have to assemble too much workflow/session/bootstrap behavior themselves when they need:
  - strict machine-output public apps
  - persistent shared service roots
  - reusable service sessions
  - multi-service integration bootstrap

Cleaned-Up Review

Keep

- Add migration tooling for older downstreams. MFM is still on the large monolithic `nixfied/project/module.nix` layout. A migration command or codemod would materially reduce downstream drift.

- Add an explicit supported mode for shared runtime roots. MFM clearly wants persistent per-project roots, and today they get that by overriding legacy-style paths manually.

- Provide a higher-level multi-service orchestration helper for CI and integration flows. MFM's `task.ci.services-start` is the clearest proof that downstreams still need to hand-build too much bootstrap logic.

- Improve the `nixfied/local/default.nix` seam. Keep the "make this clearer" half of the critique. The current wrapper/comments are easy to misread in a real downstream.

- Improve workflow/app ergonomics for strict stdout contracts. MFM's snapshot flow is a real case where wrapper tasks plus hidden tasks work, but the framework model is still awkward for public machine-output commands with service setup/teardown and handoff state.

Reframe

- Make reusable service sessions first-class.
  The real ask is not "framework has no primitives".
  The vendored framework already contains low-level policy helpers for reuse, owner scope, and discovery scope.
  The better critique is: downstreams still lack a supported session/bootstrap API built on top of those primitives.

- Expose built-in machine-readable introspection.
  The real ask is not "framework has no task/workflow/app metadata".
  The metadata already exists in the compiled model and launcher metadata.
  The better ask is a small documented JSON surface or command that directly exports:
  - app -> task/workflow resolution
  - workflow plan membership
  - selected launcher/runtime path
  - task/runtime closure membership relevant to that app
  - enough closure/debug metadata to explain why a simple app like `.#check` is pulling in unrelated package builds
  Without that, users and agents are forced to debug implementation details instead of querying supported introspection.

- Fix nested task/workflow log streaming.
  MFM's `/dev/tty` workaround is real.
  But the critique should be framed as a validated downstream pain point that needs a minimal reproducer and a framework fix, not yet as a proven universal launcher bug.

- Consider a Rust/EVM preset.
  This looks useful, but it is a product-layer preset request more than framework debt. Treat it as a reusable downstream package/preset opportunity, not as a core architectural critique.

Drop

- "Vendored metadata is richer upstream."
  This is stale for MFM's current checkout. Their `nixfied/VENDORED.txt` already contains a concrete revision and compare link.

- "Current upstream contains framework-core cleanup/fixes that MFM would likely want just by rebasing."
  This is too vague to drive a discussion. If we keep this idea at all, it needs to be rewritten as named behavioral deltas with concrete downstream impact.

- "Auto-load `nixfied/local/default.nix` by default."
  Drop the auto-load half.
  Keep the clarity/documentation half.
  Silent auto-loading would blur the current project/local boundary and make evaluation behavior less explicit.

Shortest Version

- The valid critique is not that MFM found an entirely different way to use nixfied.
- The valid critique is that MFM's real-world usage still forces them to build too much project-layer workflow/session/bootstrap shell around otherwise useful framework primitives.
- The best framework follow-ups are:
  - migration tooling
  - explicit shared-root mode
  - higher-level service-set/session bootstrap helpers
  - a clearer local extension seam
  - a small convenience introspection surface
