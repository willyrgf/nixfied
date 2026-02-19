# Repository Guidelines

## Quick Rules (Must Follow)
- Use flake apps (`nix run .#<cmd>`) as the stable interface; avoid breaking command output contracts.
- Keep CLI output plain ASCII and grep-friendly with stable prefixes: `INFO:`, `WARN:`, `ERROR:`, `OK:`, `SKIP:`.
- Prefer boring, explicit, idempotent behavior; fail fast on invalid config and avoid partial side effects.
- Keep state isolated to project/ephemeral roots; do not leak secrets in logs.
- Define tasks/workflows and exposed app surfaces in `nixfied/project/module.nix`.
- Keep exposed app names lowercase via `nixfied.tasks.<id>.ui.app.name`.
- When users name a skill (or task clearly matches one), open its `SKILL.md` and follow it for that turn.

## Project Layout
- `flake.nix`: flake entry points for apps, modules, and dev shells.
- `nixfied/project/`: project config and command model (`conf.nix`, `module.nix`).
- `nixfied/modules/`: typed module options (`core`, `runtime`, `tasks`, `workflows`, `operations`, `services/*`).
- `nixfied/compiler/`: explicit compiler passes that build `nixfiedModel.v2`.
- `nixfied/runner/`: dispatcher/executor runtime apps.
- `nixfied/registry/`: strict NDJSON event store, snapshot, replay.
- `nixfied/install/`: thin-wrapper/vendoring installer helpers.
- `nixfied/.framework/internal/`: framework install internals and `.workspace` marker handling.
- `tests/framework/`: framework v2 checks and test docs.

## Commands
- `nix run .#help`: list available commands.
- `nix run .#dev`: dev workflow.
- `nix run .#test`: tests (defaults to `ci -- --mode full --summary`).
- `nix run .#build`: build/prod workflow.
- `nix run .#check`: quality checks.
- `nix run .#format`: format workflow.
- `nix run .#ci -- --summary`: CI pipeline with concise report.
- `nix run .#test-isolation`: slot/env isolation runner.
- `nix run .#validate-env`: validate ports/dirs for current slot/env.
- `nix run .#ports`, `nix run .#check-ports`: model-derived port utilities.
- `nix run .#model`, `nix run .#stateHash`, `nix run .#tasks`, `nix run .#task::<id>`, `nix run .#schema`: introspection surfaces.
- `nix run .#run-task -- <task-id>` and `nix run .#run-workflow -- <workflow-id>`: dispatcher surfaces.
- `NIX_ENV` defaults to slot `0`; `PROJECT_ENV` defaults to `dev` unless overridden.
- Framework-only commands (requires `.workspace` marker): `framework::test`, `framework::install`.

## Coding, Testing, and PRs
- Format Nix: `find . -name '*.nix' -print0 | xargs -0 nixfmt --`.
- Common checks: `nix flake check && nix flake show && nix run .#help`.
- Framework tests: `nix run .#framework::test` or `nix run path:.#framework::test`.
- Prefer deterministic checks in `tests/framework/v2/` and keep help snapshots current.
- Keep commits small, imperative, and lowercase (for example, `expand framework test coverage`).
- PRs should include intent, affected commands/modules, test notes, and config rationale when `nixfied/project/` changes.

## Configuration
- Main project config: `nixfied/project/conf.nix`.
- Task/workflow and app behavior: `nixfied/project/module.nix` (`config.nixfied.tasks`, `config.nixfied.workflows`).
- If exposing a new app, set `ui.app.expose = true` and keep `tests/framework/v2/snapshots/help.txt` current.

## Skills
Skills are local instruction sets in `SKILL.md` files.

### Available skills
- `continual-note-taking` (`/Users/willyrgf/.codex/skills/continual-note-taking/SKILL.md`): capture durable repo notes, helper scripts, and reusable debugging/decision context.
- `skill-creator` (`/Users/willyrgf/.codex/skills/.system/skill-creator/SKILL.md`): create or update skills.
- `skill-installer` (`/Users/willyrgf/.codex/skills/.system/skill-installer/SKILL.md`): install curated skills or skills from GitHub repos.

### Skill usage rules
- Trigger: use a skill when explicitly named or when the task clearly matches its description.
- Scope: skills apply to the current turn only unless re-mentioned.
- Loading: read only what is needed from each `SKILL.md`; follow linked files progressively.
- Reuse: prefer provided `scripts/` and `assets/` over recreating content.
- Coordination: if multiple skills apply, pick the minimal set and state order.
- Fallback: if a skill is missing or unclear, report briefly and continue with the best alternative.
