# CLEANUPS

Current cleanup inventory for keeping the repository tight after the refactor wave that followed `99a0a4b`.

Current `HEAD` when this file was refreshed: `60ea450` on `2026-03-08`.

## State

- The central architecture slices from `REVIEW_BASH_NIX.md` are landed.
- The main remaining debt is no longer "unfinished Phase 2/3/4" work. It is document drift plus a small number of oversized runtime files that still mix too many concerns.
- `REVIEW_BASH_NIX.md` is now the stalest maintenance document in the repo: it still describes residual Phase 2/3/4 work that already landed.

Validation contract for any cleanup pass touching runtime behavior:

- `nix run path:.#help`
- `nix run path:.#ci -- --summary`
- `nix run path:.#framework::test`

## Hotspots By Size

These are the files most worth tightening first because they still concentrate multiple responsibilities:

- `nixfied/project/module.nix` - `2194` lines
- `nixfied/runner/executor.nix` - `1837` lines
- `nixfied/.framework/lib/process-registry.nix` - `1629` lines
- `nixfied/.framework/lib/shell-contract.nix` - `1380` lines
- `nixfied/modules/operations.nix` - `878` lines
- `nixfied/runner/workflow-modes.nix` - `826` lines
- `nixfied/.framework/lib/helpers.nix` - `728` lines

## Necessary Now

These are the cleanup items that should happen before starting another broad refactor wave.

- Rewrite `REVIEW_BASH_NIX.md`
  - remove the stale "Current highest-value remaining work" section that still claims residual Phase 2/3/4 work in `executor.nix`, PostgreSQL config, and `operations.nix`
  - remove or replace the stale Phase 2/3/4 closeout text later in the document
  - convert the file into either a closed review note or an archival design review

- Sync framework test docs with the actual flake check registry
  - `tests/framework/README.md` manually lists checks and has drifted from `tests/framework/default.nix`
  - examples currently missing from the README list include `discovery-command-surfaces-smoke`, `executor-runtime-contract`, `helpers-runtime-contract`, `workflow-modes-contract`, `service-observability-contract`, `registry-lock-recovery-smoke`, and the artifact/isolation smokes
  - either generate this list from `tests/framework/default.nix` or keep the README at a higher level and stop enumerating every check by hand

- Move long-lived review notes out of repo root once they are refreshed
  - `REVIEW_BASH_NIX.md`, `REVIEW_REPRO_RELY.md`, and this file are maintenance notes, not product entrypoints
  - after the stale review is rewritten, move review material under `docs/` or `docs/reviews/` so the root only holds active top-level surfaces

## Structural Cleanup Backlog

These are the real code cleanups still worth doing.

- `nixfied/project/module.nix`
  - extract the embedded `framework::test` runtime into its own helper/runtime file; it is a large shell program living inside the main project model file
  - add workflow constructors or shared defaults for repeated `preRun.tasks`, `postRun`, `artifacts`, and `execution` blocks used across CI and test workflows
  - promote `mkCommandTask` into a shared helper so `operations.nix` does not carry a second near-duplicate task factory
  - keep explicit service semantics, but stop mixing command factories, large shell bodies, and workflow plans in one file where possible

- `nixfied/modules/operations.nix`
  - stop hand-maintaining the service catalog as separate `postgresCfg` / `nginxCfg` / `minioCfg` / `rethCfg` / `heliosCfg` bindings plus duplicated enabled/config maps
  - replace the large `serviceSelectionPrelude` case tree with a compiled service metadata table or shared runtime helper
  - reuse the shared command-task factory from `project/module.nix` instead of keeping a local `mkTask`
  - keep probe behavior service-specific, but make selection/default-source/source-membership plumbing generic

- `nixfied/runner/executor.nix`
  - extract task dependency traversal out of `run_task()` so task graph walking is not embedded as a nested shell function
  - extract the parallel workflow scheduler into its own runtime helper; the current file still mixes task execution, scheduling, cancellation, and lock handling
  - extract workflow summary/reporting helpers (`workflow_step_records_tsv`, `workflow_steps_json`, `workflow_peak_workers`, `print_workflow_summary_report`, `write_workflow_summary_json`) into a dedicated summary runtime
  - centralize snapshot open/cleanup handling so summary/reporting paths do not repeat their own `registry_events_snapshot` / `registry_snapshot_cleanup` lifecycle

- `nixfied/runner/workflow-modes.nix`
  - stop rendering a large shell `case` accessor for every workflow/task property
  - emit one manifest or a smaller generic lookup surface so the shell side is data-driven instead of repeating accessors for each field
  - keep compiled descriptors, but reduce the amount of generated shell boilerplate

- `nixfied/.framework/lib/process-registry.nix`
  - split the file by command family: event append/emit, readers (`status`/`runs`/`slots`), mutators (`stop`/`gc`), and service inspection (`service-events`/`service-logs`/`service-status`)
  - replace the `read_kv_field` / `rewrite_kv_field` string round-trip with structured TSV/JSON rows so reconciliation does not parse its own rendered output
  - centralize repeated jq reducers such as `is_active`, `latest_by`, run/service key builders, and active-row selection
  - keep the registry contract strict, but stop repeating the same jq planning fragments in every command body

- `nixfied/.framework/lib/helpers.nix` and `nixfied/.framework/lib/process-registry.nix`
  - deduplicate owner-scope, discovery-scope, and reuse-policy shell inference
  - today both files carry their own shell wrapper around `service-policy.nix`; that logic should live in one imported runtime fragment with the policy-specific differences passed in explicitly
  - keep `helpers.nix` focused on general shell helpers and service-start ergonomics, not duplicated registry policy plumbing

- `nixfied/.framework/lib/shell-contract.nix`
  - split Nix-time validation, default-contract generation, runtime-plan emission, and bash runtime validation into separate files
  - keep one source of truth for primitive types and runtime primitives, but reduce the number of unrelated responsibilities inside this one file
  - decide whether `mkServiceRuntimePrimitivesV1` is still needed; if not, remove it, and if yes, add a direct consumer or tighter contract coverage so it is not dead weight

## Optional Follow-Ups

These are worthwhile only after the items above are done.

- Recreate the old counting helper if exact hotspot totals still matter
  - the historical `python3 .codex/helpers/review_bash_nix.py` path is gone
  - a small reproducible counter could refresh review-doc size numbers when needed

- Minor supervisor UX cleanup
  - `nixfied/.framework/supervisor/status.nix`
  - focus only on concrete log-view or wrapper behavior wins

- Service-specific lifecycle polish where semantics justify it
  - `nixfied/.framework/postgres/lifecycle.nix`
  - `nixfied/.framework/minio/lifecycle.nix`
  - only touch these when the change is about service semantics, not abstracting a tiny wrapper

- Clarify the help-vs-introspection distinction in docs
  - `nix run .#help` shows top-level user app surfaces
  - docs also mention introspection apps such as `model`, `stateHash`, `tasks`, `services`, `task::<id>`, and `schema`
  - if that distinction is intentional, document it in one place and keep the wording consistent

## What Not To Chase

- Do not reopen Phase 2/3/4 as if they are still unfinished. Those specific review slices are already landed.
- Do not churn service lifecycle files just to remove tiny wrappers.
- Do not abstract every explicit service field mapping if it makes service-specific behavior harder to read.
- Do not rewrite `supervisor/status.nix` unless there is a concrete behavior or reuse win.

## Landed Context

The post-`99a0a4b` cleanup wave was real and materially reduced shared runtime debt.

Key central commits:

- `70edfde` `refactor: plumb compiled discovery command surfaces`
- `0a358d2` `refactor: normalize remaining service lifecycle scaffolding`
- `34a75ac` `refactor: compile service probe plans for operations`
- `bf35866` `refactor: split install runtime policy helpers`
- `de8fcc4` `refactor: split helper cleanup and fixture runtimes`
- `a394f3e` `refactor: extract test isolation runtime`
- `0cf2996` `refactor: split executor runtime helpers`
- `f0bdaa6` `refactor: compile workflow unit plans`
- `ba90361` `refactor: compile executor task descriptors`
- `d5340c5` `refactor: compile postgres config artifacts`
- `d89fe9b` `refactor: extract operations probe runtime`
- `60ea450` `refactor: share managed service ready outcome helper`

For the full range, use:

- `git log --oneline 99a0a4b..60ea450`

## Next Time

- update `REVIEW_BASH_NIX.md` before starting more cleanup work
- treat this file as the current cleanup ledger, not as a second historical review doc
- only start another cleanup wave if it removes a real shared seam or eliminates real doc drift
