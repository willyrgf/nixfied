# Repository Guidelines

## Project Structure & Module Organization
- `flake.nix` defines the Nix flake entry points and wires apps, modules, and dev shells.
- `nixfied/` contains framework implementation:
  - `nixfied/.framework/` holds vendored framework code and framework-only marker (`.workspace`).
  - `nixfied/project/` holds project configuration (`conf.nix`, command files like `dev.nix`, `test.nix`, `prod.nix`, `quality.nix`, `ci.nix`).
  - `nixfied/.framework/internal/` defines flake apps (core, install, test, isolation, module-apps).
  - `nixfied/.framework/lib/` contains helper modules (builders, helpers, summary, run-registry, parallel, process, port-utils).
  - `nixfied/.framework/ci.nix`, `nixfied/.framework/slots.nix`, `nixfied/.framework/hooks.nix`, `nixfied/.framework/ephemeral.nix` implement core framework features.
  - `nixfied/.framework/postgres/`, `nixfied/.framework/nginx/`, `nixfied/.framework/supervisor/`, `nixfied/.framework/minio/` are framework module directories.
- `tests/framework/` contains fixtures and documentation for framework tests.

## Build, Test, and Development Commands
Use the flake apps; they are the primary interface:
- `nix run .#help` - list available commands.
- `nix run .#dev` - run the dev workflow (placeholder unless customized).
- `nix run .#test` - run tests (delegates to `nix run .#ci -- --mode full --summary` by default).
- `nix run .#build` - build/prod workflow (placeholder unless customized).
- `nix run .#check` - quality checks.
- `nix run .#ci` - CI pipeline runner; add `-- --summary` for a concise report.
- `nix run .#test-isolation` - run the isolation runner across slots/envs (configured in `nixfied/project/conf.nix`).
- `nix run .#validate-env` - validate ports/dirs for the current slot/env (used by the isolation runner).
- Module/utility apps (vary by enabled modules): `nix run .#ports`, `nix run .#check-ports`, `nix run .#up`, `nix run .#down`.
- Slot/env-sensitive module and supervisor apps require explicit `PROJECT_ENV`; `NIX_ENV` defaults to slot `0` when unset (for example: `up`, `down`, `svc-*`, `svc::*`).
- Framework-only (requires `nixfied/.framework/.workspace` marker): `nix run .#framework::test`, `nix run .#framework::install`, `nix run .#framework::upgrade`, `nix run .#framework::prompt-plan`.

Additional repo checks used by contributors:
- Formatter: `find . -name '*.nix' -print0 | xargs -0 nixfmt --`.
- Checks: `nix flake check && nix flake show && nix run .#help`.
- Tests: `nix run .#framework::test`.

## Coding Style & Naming Conventions
- Nix is the primary language; follow the existing formatting and structure in `nixfied/`.
- Keep command definitions in `nixfied/project/*.nix` using the `commands.<name>` schema.
- Use descriptive, lowercase command names (e.g., `dev`, `test`, `build`, `check`).

## Output & Logging
- No emojis and no non-ASCII markers in CLI output/logs; keep output plain ASCII and grep-able. Prefer stable prefixes at line start: `INFO:`, `WARN:`, `ERROR:`, `OK:`, `SKIP:`.
- Prefer single-line events that include context as `key=value` (example: `INFO: starting service name=postgres slot=0 env=dev port=5432`).
- Errors go to stderr and should include an actionable next step when possible.

## Engineering Principles
- Prefer boring, explicit solutions over clever abstractions.
- Keep interfaces stable: flake apps (`nix run .#<cmd>`) and their outputs are a contract; if output changes, keep stable prefixes and avoid breaking grep/script usage.
- Write idempotent scripts and commands: re-running should be safe and should converge to the desired state.
- Fail fast on invalid config/env; avoid partial side effects on failure.
- Keep state isolated: use slots/ephemeral execution where appropriate; avoid writing outside project/ephemeral roots unless explicitly required.
- Do not leak secrets into logs; never echo `.env` contents or credentials.

## Testing Guidelines
- Framework tests live in `tests/framework/` with fixtures under `tests/framework/fixtures/`.
- Run tests via `nix run .#framework::test` or `nix run path:.#framework::test` for untracked changes.
- Prefer adding fixtures or minimal scripts that exercise helper functions and CI behavior.

## Commit & Pull Request Guidelines
- Commit messages in this repo are short, imperative, and lowercase (e.g., "expand framework test coverage").
- Keep commits small and focused (one logical change per commit) so upgrade history stays useful in `nixfied/UPGRADE_CHECK.txt` (`git log --oneline` and `git diff --stat` between framework revisions when resolvable).
- PRs should include:
  - A brief description of intent and affected commands/modules.
  - Notes on testing (`nix run .#test`, `nix run .#framework::test`, or "not run").
  - Any config changes in `nixfied/project/` with rationale.

## Configuration Tips
- Project configuration lives in `nixfied/project/conf.nix`; enable modules (Postgres/Nginx) there.
- Command behavior is defined in `nixfied/project/*.nix`; update `nixfied/project/default.nix` if you add new files.

## Skills
A skill is a set of local instructions to follow that is stored in a `SKILL.md` file. Below is the list of skills that can be used. Each entry includes a name, description, and file path so you can open the source for full instructions when using a specific skill.

Note: this section reflects the current Codex environment and may differ across machines/sessions.

### Available skills
- continual-note-taking: Continually capture durable repo notes and small helper scripts while you work: command recipes, gotchas, decisions, debugging playbooks, and reusable snippets. Use when the user asks to take notes, document what you learned, improve future workflows, create helpers, or when you want knowledge to compound across Codex sessions. (file: /Users/willyrgf/.codex/skills/continual-note-taking/SKILL.md)
- skill-creator: Guide for creating effective skills. This skill should be used when users want to create a new skill (or update an existing skill) that extends Codex's capabilities with specialized knowledge, workflows, or tool integrations. (file: /Users/willyrgf/.codex/skills/.system/skill-creator/SKILL.md)
- skill-installer: Install Codex skills into $CODEX_HOME/skills from a curated list or a GitHub repo path. Use when a user asks to list installable skills, install a curated skill, or install a skill from another repo (including private repos). (file: /Users/willyrgf/.codex/skills/.system/skill-installer/SKILL.md)

### How to use skills
- Discovery: The list above is the skills available in this session (name + description + file path). Skill bodies live on disk at the listed paths.
- Trigger rules: If the user names a skill (with `$SkillName` or plain text) OR the task clearly matches a skill's description shown above, you must use that skill for that turn. Multiple mentions mean use them all. Do not carry skills across turns unless re-mentioned.
- Missing/blocked: If a named skill isn't in the list or the path can't be read, say so briefly and continue with the best fallback.
- How to use a skill (progressive disclosure):
  1) After deciding to use a skill, open its `SKILL.md`. Read only enough to follow the workflow.
  2) When `SKILL.md` references relative paths (e.g., `scripts/foo.py`), resolve them relative to the skill directory listed above first, and only consider other paths if needed.
  3) If `SKILL.md` points to extra folders such as `references/`, load only the specific files needed for the request; don't bulk-load everything.
  4) If `scripts/` exist, prefer running or patching them instead of retyping large code blocks.
  5) If `assets/` or templates exist, reuse them instead of recreating from scratch.
- Coordination and sequencing:
  - If multiple skills apply, choose the minimal set that covers the request and state the order you'll use them.
  - Announce which skill(s) you're using and why (one short line). If you skip an obvious skill, say why.
- Context hygiene:
  - Keep context small: summarize long sections instead of pasting them; only load extra files when needed.
  - Avoid deep reference-chasing: prefer opening only files directly linked from `SKILL.md` unless you're blocked.
  - When variants exist (frameworks, providers, domains), pick only the relevant reference file(s) and note that choice.
- Safety and fallback: If a skill can't be applied cleanly (missing files, unclear instructions), state the issue, pick the next-best approach, and continue.
