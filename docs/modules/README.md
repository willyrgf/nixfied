# Module Documentation

These pages document service configuration knobs in `nixfied/project/conf.nix`.

The current framework surface does not expose dedicated module command namespaces. Module behavior is consumed through shared tasks/workflows and model-derived operations.

Use `nix run .#help` for the live app list.

## Configuration Source of Truth

- Service enablement, ports, and source catalogs: `nixfied/project/conf.nix`
- Service option schemas: `nixfied/modules/services/*.nix`
- Project-layer composition and shared builders: `nixfied/project/module.nix`
- Project runtime/services/tasks/workflows: `nixfied/project/{runtime,services,tasks,workflows}.nix`
- Framework-owned task/workflow presets: `nixfied/framework/presets/*.nix`

## Operational Validation

- `nix run .#health -- --service <name|all> [--source <key>]` runs health checks for selected enabled services.
- `nix run .#ready -- --service <name|all> [--source <key>]` runs readiness checks for selected enabled services.
- `nix run .#validate-env` checks slot/env constraints.
- `nix run .#ports` prints computed per-slot/per-env ports.
- `nix run .#check-ports` reports current listener status.

## Module Pages

- `docs/modules/postgres.md`
- `docs/modules/nginx.md`
- `docs/modules/minio.md`
- `docs/modules/reth.md`
- `docs/modules/helios.md`

## Service Process Safety

- Probe the intended service endpoint directly; do not validate Helios using a generic execution RPC alias.
- Stop only owned service processes (PID files/process groups), never arbitrary listeners discovered only by port.
- Prefer model-generated operations (`health`, `ready`, `stop-run`, `stop-all-runs`) over ad hoc service control.
- Keep shell output prefix-stable and grep-friendly: `INFO:`, `WARN:`, `ERROR:`, `OK:`, `SKIP:`.
