# Adapters

An adapter is a Nix module that compiles one concrete service into the generic
model primitives. The runtime never learns the domain — an adapter is pure
model authorship, importable by any project:

```nix
{ adapters, ... }:
{
  imports = [ adapters.postgres ];
  nixfied.project.projectId = "my-project";
  nixfied.placement.ports.base = 24580;
}
```

`adapters` is provided to every compiled `nixfied.nix` via `specialArgs`;
the registry lives in `nix/adapters/default.nix` (`synthetic`, `postgres`,
`reth`).

## What an adapter provides

| Piece | Contract |
| --- | --- |
| Closures | One per executable, with declared `effects` (the one hand-declared attestation). `operationBindings` are derived from the invocation graph; declare a list only as a narrowing gate. |
| Service | A lifecycle of inline invocations: `prepare` (a **task reference** — see below) / `start` / `ready` / `health` / `stop` / `clean`, endpoints where the service listens, `stateRefs = [ "slot" ]`, containment. Operation ids and terminal tokens are derived; declare only to override (postgres overrides `stop.signal`). |
| Tasks | Named smoke/verification tasks (`smoke-query`, `reth-smoke`). **This is how adapter checks enter an adopter's pipeline**: the adopter references them as steps in its own composites — membership does not exist, so importing an adapter contributes *definitions only* and adds zero startup to tasks that require nothing of it. |

There is no Execs piece (invocations are inline and anonymous — reuse is a
Nix `let`) and no Environment piece (membership died with the composition
rewrite).

## Conventions

- **State layout**: all service state lives under `${stateDir}/<service>` (e.g.
  `${stateDir}/pgdata`, `${stateDir}/reth`), where `${stateDir}` is the
  runtime-materialised slot state root — so marker-gated `clean` owns it and an
  epoch upgrade can rebuild it.
- **Prepare is a task**: bind `lifecycle.prepare.task` to a declared task
  (leaf or composite) — full task semantics, ordinary task evidence, and
  cross-service `requires` for typed initialization (the combined
  `connectsTo` + prepare-requires graph must stay acyclic).
- **Idempotent prepare**: a second run on the same slot must adopt existing
  state, not fail in init. When the package's own init tool is not idempotent
  (initdb), wrap it in a `writeShellApplication` closure that detects and
  adopts a complete data dir and rebuilds an incomplete one (see
  `nix/adapters/postgres.nix`).
- **Protocol probes**: ready/health should be `kind = "exec"` protocol probes
  (`pg_isready`, a JSON-RPC call via `curl`) rather than tcp connects, so
  "ready" means the service answers, not that the port is bound.
- **Socket paths**: anything that opens a Unix socket must keep the path short
  (macOS `sun_path` limit under deep state dirs): disable the socket
  (postgres: `unix_socket_directories=`) or place it under `/tmp` keyed by the
  service port and remove a stale one before start (reth IPC).
- **Platform behavior** is the adapter's job, decided at Nix eval time
  (`pkgs.stdenv.hostPlatform.isDarwin`): e.g. postgres pins
  `shared_memory_type = mmap` on Darwin at init and verifies it on adoption.
- **Stop semantics** must match the service's protocol: postgres uses `INT`
  (fast shutdown — `TERM` is smart shutdown and waits for clients); reth uses
  `TERM` with a longer timeout.

## Parameterization

Adapters are ordinary modules: the importing project overrides options by
module merging — `nixfied.services.postgres.lifecycle.stop.signal`,
`nixfied.placement.ports.base`, probe timing, or lifecycle invocations. No
adapter has its own option namespace.

## Multiple listeners: model every endpoint

A service that binds more than one listener (reth: http/ws/authrpc) declares each
as a named `endpoint` and names the primary with `primaryEndpoint` (see
`nix/adapters/reth.nix`). The planner reserves a contiguous port block — one port
per endpoint — so every listener is reserved, conflict-checked against other
services and slots, and ownership-verified after readiness. The start wrapper
receives the planned ports as arguments (`${port:reth-http}`, `${port:reth-ws}`, …)
and derives nothing.

Do **not** derive auxiliary ports (`+1/+2/+3`) inside the wrapper: a derived port
is outside the plan, so it is neither reserved nor isolated across slots. Model it
as an endpoint instead. A listener the adapter cannot model (no fixed offset, or an
out-of-band socket) must be disabled rather than left unreserved — see reth's
`--ipcdisable`.

## Endpoint-less services: durable is not listening

A daemon that binds nothing — a queue consumer, an indexer, a worker that only
connects OUT — declares **no** endpoint form at all. It keeps the full durable
contract (owned start, probed readiness, containment, marker-gated clean) with
the consequences the validators enforce:

- ready/health must be **invocation probes** (a tcp probe has no target);
  readiness means "the probe answers" — a heartbeat file under `${stateDir}`,
  a queue-depth query through the broker it connects to;
- nothing may address it: `${port:<id>}`/`${host:<id>}` toward it are rejected
  in every scope, and bare `${port}`/`${host}` in its own lifecycle are too;
- its start closure must **not** attest `network-listener` (effects
  coherence — an unreservable listener is the bypass this document already
  forbids);
- `requires`/`connectsTo` **toward** it stay legal: ready ordering, failure
  semantics, and the derived service union — minus addressability.

See the worker in `examples/toolchain/nixfied.nix` for the standing shape.
