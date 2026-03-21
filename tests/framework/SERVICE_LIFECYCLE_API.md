# Service Lifecycle API Review

This document records the current service lifecycle surface and the framework
tests that cover it.

## Naming

- For the public service modules in this repository, "up/down" means
  `start/stop`.
- `supervisor` is a separate runtime surface and should not be treated as part
  of the same public lifecycle matrix.

## Public Service Matrix

| Service | start | stop | restart | health | ready | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| postgres | yes | yes | yes | yes | yes | Also exposes `ready-test`. |
| nginx | yes | yes | yes | yes | yes | Also exposes `reload`. |
| minio | yes | yes | yes | yes | yes | Default health/ready plans are HTTP probes. |
| reth | yes | yes | yes | yes | yes | Default health/ready plans are JSON-RPC probes. |
| helios | yes | yes | yes | yes | yes | Default ready is stricter than health. |

## Canonical Probe Overrides

For the public five services, `health` and `ready` now come from one
canonical probe model:

- `nixfied.services.<name>.probes.health`
- `nixfied.services.<name>.probes.ready`

If a probe block is absent, the framework uses the built-in default plan for
that service and mode.

Each probe block supports:

- `strategy = "replace" | "prepend" | "append"`
- `steps = [ ... ]`
- optional `wait = { enabled, timeoutSeconds, intervalSeconds }`

Supported step kinds today:

- `tcp`
- `http`
- `jsonrpc`
- `postgres-pg-isready`
- `postgres-query`
- `helios-ready`
- `exec`

Both aggregate ops and direct service lifecycle checks consume the same
normalized probe plan:

- aggregate `task.ops.health`
- aggregate `task.ops.ready`
- direct service `health`
- direct service `ready`

Workflow `preRun` / `postRun` probe phases scope the aggregate ops to the
workflow unit closure service set instead of all enabled services.

The important behavioral split is `wait`:

- aggregate `task.ops.health` and `task.ops.ready` stay one-shot checks
- direct service `ready` honors `ready.wait`

`exec` probe steps receive a stable env contract:

- `NIXFIED_PROBE_MODE`
- `NIXFIED_PROBE_SERVICE`
- `NIXFIED_PROBE_SOURCE`
- `NIXFIED_PROBE_<ENDPOINT>_PORT`

Direct service `ready` may execute a successful `exec` probe more than once
when `wait` is enabled, because the wait loop retries silently and then reruns
the probe for visible success output.

## Supervisor Surface

`supervisor` currently exposes `start`, `startDaemon`, `stop`, `restart`,
`health`, and `status`.

It does not expose `ready`, and downstream code should not assume that it does.

## Existing Coverage

- Shared lifecycle scaffolding is covered by the managed lifecycle contract.
- Service status formatting is covered by the service observability contract.
- Aggregate `task.ops.ready` / `task.ops.health` behavior is covered by
  readiness and shutdown smokes.
- Canonical probe plan normalization and override merging are covered by the
  service probe override contract.
- Aggregate and direct service probe override behavior is covered by the
  service probe override smoke.

## Coverage Added In This Pass

- A service API surface contract locks the lifecycle matrix for the public five
  services.
- A direct lifecycle smoke exercises `start`, `health`, `ready`, `restart`,
  and `stop` for each public service.
- A dedicated supervisor smoke exercises the daemonized supervisor lifecycle
  surface separately.
