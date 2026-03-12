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
| minio | yes | yes | yes | yes | yes | Health/ready are HTTP endpoint probes. |
| reth | yes | yes | yes | yes | yes | Health/ready are JSON-RPC probes. |
| helios | yes | yes | yes | yes | yes | Ready is stricter than health. |

## Supervisor Surface

`supervisor` currently exposes `start`, `startDaemon`, `stop`, `restart`,
`health`, and `status`.

It does not expose `ready`, and downstream code should not assume that it does.

## Existing Coverage

- Shared lifecycle scaffolding is covered by the managed lifecycle contract.
- Service status formatting is covered by the service observability contract.
- Aggregate `task.ops.ready` / `task.ops.health` behavior is covered by
  readiness and shutdown smokes.

## Coverage Added In This Pass

- A service API surface contract locks the lifecycle matrix for the public five
  services.
- A direct lifecycle smoke exercises `start`, `health`, `ready`, `restart`,
  and `stop` for each public service.
- A dedicated supervisor smoke exercises the daemonized supervisor lifecycle
  surface separately.
