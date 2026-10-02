# Adapters

## Import and compose adapters

An adapter is a Nix module that compiles one concrete service into the generic
manifest primitives. The runtime never learns the domain — an adapter is pure
manifest authorship, importable by any project:

[`OPTIONS.md`](OPTIONS.md) is the generated reference for the exact option
types and defaults used below.

```nix
{ adapters, ... }:
{
  imports = [ adapters.postgres ];
  nixfied.project.projectId = "my-project";
  nixfied.placement.ports.base = 24580;
}
```

`adapters` is provided to every compiled `nixfied.nix` via `specialArgs`.
Discover the published modules and read their contributions from their
definitions:

```sh
nix run .#docs -- api module
nix run .#docs -- api module adapter/postgres
```

## What an adapter provides

| Piece | Contract |
| --- | --- |
| Closures | One per executable, with declared `effects` (the one hand-declared attestation). |
| Service | A lifecycle of inline invocations: `prepare` (a **task reference** — see `docs topic services`) / `start` / `ready` / `health` / `stop` / `clean`, endpoints where the service listens, state labels, containment. Operation ids and terminal tokens are derived; declare only to override (postgres overrides `stop.signal`). |
| Tasks | Named smoke/verification tasks (`smoke-query`, `reth-smoke`). **This is how adapter checks enter an adopter's pipeline**: the adopter references them as steps in its own composites — membership does not exist, so importing an adapter contributes *definitions only* and adds zero startup to tasks that require nothing of it. |

There is no Execs piece (invocations are inline and anonymous — reuse is a
Nix `let`) and no Environment piece (membership died with the composition
rewrite).

## Conventions

- **State layout**: all service state lives under `${stateDir}/<service>` (e.g.
  `${stateDir}/pgdata`, `${stateDir}/reth`), where `${stateDir}` is the
  runtime-materialised slot state root — so marker-gated `clean` owns it and an
  explicit cleanup can remove it.
- **Prepare is a task**: bind `lifecycle.prepare.task` to a declared task
  (leaf or composite) — full task semantics, ordinary task evidence, and
  cross-service `requires` for typed initialization (the combined
  `connectsTo` + prepare-requires graph must stay acyclic).
- **Idempotent prepare**: a second run on the same slot must adopt existing
  state, not fail in init. When the package's own init tool is not idempotent
  (initdb), wrap it in a `writeShellApplication` closure that detects and
  adopts a complete data dir and rebuilds an incomplete one (see
  `nix/adapters/postgres.nix`).
- **Protocol probes**: every endpoint declares `readyProbe` and `healthProbe`
  invocations that use its assigned address and check meaningful protocol
  behavior. Postgres uses `pg_isready`; Reth checks HTTP JSON-RPC, a complete
  WebSocket exchange, JWT-authenticated Engine API JSON-RPC, and native RLPx
  handshakes when an adopter enables a peer listener. A listener
  alone cannot make a service ready. Keep stdin null and omit invocation
  timeouts; `lifecycle.ready.policy` and `lifecycle.health.policy` own
  attempt deadlines and whole-round retry limits.
- **Tool acceleration**: cache/build state is child/tool/project-owned, not an
  adapter or runtime resource. Adapter-provided checks may pass an ordinary
  declared environment value or argument, but Nixfied does not place, create,
  report, retain, or selectively clean the resulting artifacts.
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

## Endpoints and placeholders

Placeholders in invocation arguments and environment values are scope-specific:

| Invocation owner | Bare `${port}` / `${host}` | Named `${port:<name>}` / `${host:<name>}` |
| --- | --- | --- |
| Leaf task (including a service prepare task) | Primary endpoint of the **first** service in its authored `requires` list | Primary endpoint of a directly declared required **service id**, not an endpoint id |
| Service start | The service's own primary endpoint | An own endpoint id first, then the primary endpoint of a directly declared `connectsTo` service id |
| Attached endpoint probe | That endpoint, including a nonprimary endpoint | Same directly declared service scope as start |
| Endpoint-less scalar probe | Invalid | Primary endpoint of a directly declared `connectsTo` service |

Endpoint shorthand declares a single primary endpoint. With an `endpoints` map,
`primaryEndpoint` must name one of its keys. Task bare references do not select
by sorted order or skip an endpoint-less first dependency. Transitive derived
requirements do not expand the named placeholder scope. Endpoint-less services
may be dependencies but cannot be addressed by endpoint placeholders.

`${stateDir}` names the runtime-materialised slot root in either invocation
scope. `${secret:<id>}` is allowed only in invocation environment values, must
name a declared secret, and is rejected in argv. The environment stays hermetic.
Use escaped interpolation in Nix strings, for example `"\${port:postgres}"`, or
`''${port:postgres}` inside an indented Nix string. Arbitrary `${...}` child-tool
syntax is not a registry of Nixfied placeholders; consult these exact supported
forms rather than inferring additional names from examples.

Authored text is scanned from left to right. `${HOME:-${port}}` retains the
unknown outer child syntax and substitutes the inner `${port}`. `${portfoo}`
remains literal. Empty named payloads, braces inside a named payload (such as
`${port:${HOME}}`), and unclosed recognized references reject. Secrets remain
forbidden anywhere in authored argv, including inside unknown child syntax.
Inserted values are opaque: a secret containing `${secret:other}` or a state
path containing `${port}` is inserted literally and is never substituted again.
`run[0]` is the literal executable basename used for tool selection; templates
apply to the argument tail and environment values. Secret references are forbidden
throughout `run`, including the executable position.

## Multiple listeners: declare every endpoint

A service that binds more than one listener (reth: http/ws/authrpc, plus peers
when enabled) declares each
as a named `endpoint` and names the primary with `primaryEndpoint` (see
`nix/adapters/reth.nix`). The planner reserves a contiguous port block — one port
per endpoint — so every listener is reserved, coordinated across independent
state roots by host endpoint locks, and checked for an exact managed listener
plus its application probe in each ready and health round. Attach both probes
to every endpoint. A wildcard-only listener cannot satisfy an exact declaration;
successful bind preflight is not a host-wide absence or exclusivity proof.
The start wrapper
receives the planned ports as arguments (`${port:reth-http}`, `${port:reth-ws}`, …)
and derives nothing.

Do **not** derive auxiliary ports (`+1/+2/+3`) inside the wrapper: a derived port
is outside the plan, so it is neither reserved nor isolated across slots. Declare it
as an endpoint instead. A listener the adapter cannot represent (no fixed offset, or an
out-of-band socket) must be disabled rather than left unreserved — see reth's
`--ipcdisable`.

### Reth peer listeners

The default Reth adapter runs a peerless `--dev` node with three endpoints.
Adopters that enable peering must declare the peer TCP endpoint as well. The
adapter's existing `reth-rpc-probe` closure supplies
`nixfied-reth-probe peer HOST PEER_PORT HTTP_PORT`: it reads `admin_nodeInfo`
from the supplied HTTP port and asks `reth p2p rlpx ping` to perform ECIES
authentication and the devp2p Hello exchange against the supplied peer port.
Only the public key is taken from the returned enode; its advertised address
and ports never choose the connection target. Failed HTTP, malformed identity,
or failed handshake exits nonzero with a fixed diagnostic and no response text.

Attach the same invocation to readiness and health on the peer endpoint:

```nix
let
  peerProbe = {
    tools = [ "reth-rpc-probe" ];
    run = [
      "nixfied-reth-probe" "peer" "\${host}" "\${port}"
      "\${port:reth-http}"
    ];
  };
in {
  nixfied.services.reth.endpoints.reth-p2p = {
    readyProbe = peerProbe;
    healthProbe = peerProbe;
  };
}
```

This accompanies an adopter-owned start invocation that binds `--addr` to
`${host:reth-p2p}` and `--port` to `${port:reth-p2p}`, enables `admin` on HTTP
(`--http.api eth,admin`), and allows at least one inbound peer. The HTTP and peer
listeners must use the same loopback host for this probe. Disable discovery
(`--disable-discovery`) so it adds no unreserved UDP listener, and disable any
other listeners the custom service does not declare. Keep the ordinary HTTP
probe attached to its HTTP endpoint; the peer handshake checks a separate
listener. Lifecycle policies own probe deadlines and retries.

## Endpoint-less services: durable is not listening

A daemon that binds nothing — a queue consumer, an indexer, a worker that only
connects OUT — declares **no** endpoint form at all. It keeps the full durable
contract (owned start, probed readiness, containment, marker-gated clean) with
the consequences the validators enforce:

- ready/health each declare a scalar **`probe` invocation** and a `policy`;
  readiness means "the probe answers" — a heartbeat file under `${stateDir}`,
  a queue-depth query through the broker it connects to;
- nothing may address it: `${port:<id>}`/`${host:<id>}` toward it are rejected
  in every scope, and bare `${port}`/`${host}` in its own lifecycle are too;
- its start closure must **not** attest `network-listener` (effects
  coherence — an unreservable listener is the bypass this document already
  forbids);
- `requires`/`connectsTo` **toward** it stay legal: ready ordering, failure
  semantics, and the derived service union — minus addressability.

See the worker in `examples/toolchain/nixfied.nix` for the reference shape.
