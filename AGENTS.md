# Repository Guidelines

## Quick Rules (Must Follow)
- Use flake apps (`nix run .#<cmd>`) as the stable interface; avoid breaking command output contracts.
- Keep CLI output plain ASCII and grep-friendly with stable prefixes: `INFO:`, `WARN:`, `ERROR:`, `OK:`, `SKIP:`.
- Prefer boring, explicit, idempotent behavior; fail fast on invalid config and avoid partial side effects.
- Keep state isolated to project/ephemeral roots; do not leak secrets in logs.
- Use lowercase command names in `nixfied/project/*.nix` via `commands.<name>`.
- When users name a skill (or task clearly matches one), open its `SKILL.md` and follow it for that turn.

## Project Layout
- `flake.nix`: flake entry points for apps, modules, and dev shells.
- `nixfied/.framework/`: vendored framework code and `.workspace` marker.
- `nixfied/project/`: project config and command model (`conf.nix`, `module.nix`).
- `nixfied/.framework/internal/`: flake app wiring (core, install, test, isolation, module apps).
- `nixfied/.framework/lib/`: framework helpers (builders, helpers, summary, run registry, parallel, process, port utils).
- `nixfied/.framework/{ci,slots,hooks,ephemeral}.nix`: core framework behavior.
- `nixfied/.framework/{postgres,nginx,supervisor,minio}/`: module directories.
- `tests/framework/`: framework v2 checks and test docs.

## Commands
- `nix run .#help`: list available commands.
- `nix run .#dev`: dev workflow.
- `nix run .#test`: tests (defaults to `ci -- --mode full --summary`).
- `nix run .#build`: build/prod workflow.
- `nix run .#check`: quality checks.
- `nix run .#ci -- --summary`: CI pipeline with concise report.
- `nix run .#test-isolation`: slot/env isolation runner.
- `nix run .#validate-env`: validate ports/dirs for current slot/env.
- `nix run .#ports`, `nix run .#check-ports`, `nix run .#up`, `nix run .#down`: module/utility apps (when enabled).
- `PROJECT_ENV` is required for slot/env-sensitive module and supervisor apps; `NIX_ENV` defaults to slot `0`.
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
- Command behavior: `nixfied/project/*.nix`.
- If adding command files, update `nixfied/project/default.nix`.

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
