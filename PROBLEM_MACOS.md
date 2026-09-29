# macOS 27 TCP listener inventory and endpoint ownership

Status: investigated locally on macOS 27.0.1 (build 26A434, Darwin 27.0.0),
Apple Silicon, 29 September 2026. The endpoint integration proof is not passing
on this host. This is an investigation and design gate, not a change to shipped
behavior. See also the
[`privileged observer design review`](docs/PORT_OBSERVER_DESIGN_REVIEW.md).

## The guarantee at risk

[PORT-1](docs/CONTRACT.md#source-identity-and-endpoints) requires an
endpoint-backed service to establish exact kernel-observed ownership before it
is declared ready. Before starting a child, the runtime must exclude stable
exact and wildcard listeners that conflict with each declared endpoint. The
host endpoint lock serializes Nixfied starts, but cannot complete an incomplete
host inventory or constrain unrelated processes. A successful bind, TCP
connection, process name, or empty partial inventory does not prove PORT-1.

The runtime owns admission and readiness. The macOS observer currently reads
`net.inet.tcp.pcblist_n`, parses the `xinpgen` envelope and PCB records, and
correlates listener identities with process FDs. If preflight cannot establish
a complete snapshot, admission returns `PORT_UNVERIFIABLE` before service
preparation. An incomplete readiness observation refuses the ready commit with
the same error. These are contract requirements, not test-only expectations.

## How the failure surfaced

The first `nix run .#ci` attempt stopped in the Darwin `rust-workspace` check:
Rust 1.96 Clippy, run with `-D warnings`, rejected an explicit lifetime on
`parse_record` and a manual `%` multiple-of check in
[`macos.rs`](runtime/crates/nixfied-runtime/src/service/endpoint/macos.rs).
Local commit `8f65fa0e` fixed those source issues and added a conservative PCB
record-count discrepancy check. `nix flake check --no-write-lock-file` then
passed. The wider `nix run .#ci -- --dirty` reached macOS endpoint integration
and failed eleven endpoint tests because the observer could not prove a
complete listener inventory. Clippy exposed, but did not cause, the runtime
problem.

The current check compares decoded PCB records with the `xinpgen` header and
trailer counts. It retries and then returns `PORT_UNVERIFIABLE` on a
discrepancy. This is a conservative rejection signal. Count agreement is not
proof of host-wide visibility, and count disagreement is not necessarily
proof of a missing *listener*: public XNU
[`in_pcblist.c`](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/netinet/in_pcblist.c)
can skip dead or initializing PCBs while emitting records. That public source
does not certify the implementation in the installed macOS 27 kernel.

## Unprivileged listener observations

The original standalone probe at `/private/tmp/nixfied_tcp_inventory_probe.py`
held a temporary `127.0.0.1` TCP listener in the observing process and another
in a child. It read `pcblist_n` five times, decoding its six subrecords and
`xinpgen` envelope. No Nixfied runtime code participated.

| Execution context | `xinpgen` count | Decoded PCB records | Observer listener | Child listener |
| --- | ---: | ---: | --- | --- |
| Direct command | 27–28 | 1 | Present | Absent |
| Apple Terminal | 29 | 1 | Present | Absent |
| Independent `launchctl submit` job, parent PID 1 | 35 | 1 | Present | Absent |

Counts varied between runs. The stable result was that the returned body held
only the caller's listener while its child still listened. The separate
Terminal and launchd executions rule out a restriction confined to the Codex
process tree. They do not identify the precise kernel access policy.

On another sample, `pcblist_n` advertised 41 records and returned one; Apple's
`nettop -m tcp -n -L 1 -s 1` displayed both controlled listeners and listeners
attributed to launchd. This corroborated the missing child in the unprivileged
PCB body. `nettop` cannot serve as PORT-1's authority: its rendered output has
no documented complete atomic inventory, stable socket identity, complete
holder list, or IPv6-only bind-mode evidence.

The checked-in
[`tests/macos/prove_observer_feasibility.py`](tests/macos/prove_observer_feasibility.py)
repeated the baseline. In one five-sample run, the `xinpgen` counts were 50–51,
one PCB was decoded, the observing process's listener was present, and its
stable child listener was absent every time.

## Privileged observations supplied by the user

An initial `sudo -n` attempt stopped at `sudo: a password is required`. The
subsequent local terminal runs with authentication resolved that unknown:

1. `sudo -v && /usr/bin/python3 /private/tmp/nixfied_privileged_observer_probe.py
   --sudo-sysctl` kept the Python harness unprivileged and elevated only
   Apple's `/usr/sbin/sysctl`. Five samples decoded 51 of 51 advertised PCBs.
   Both controlled listeners were present, and the child's PCB socket handle
   matched its FD handle.
2. `sudo /usr/bin/python3 /private/tmp/nixfied_privileged_observer_probe.py
   --as-root` ran the observer as root and dropped its child listener to UID
   501. In five samples both listeners were present and the child FD
   correlated. Each sample advertised 49 PCBs but decoded 47, with matching
   header/trailer generation and count. Root visibility therefore does not
   make the current count-equality check pass.
3. A later root run again saw and correlated both controlled listeners in all
   five samples. Its full process scan listed and inspected 1,239 PIDs,
   counted 687 socket FDs, and reported zero uninspectable live processes. The
   comparable unprivileged scan found 323 uninspectable live processes.

These runs establish cross-UID positive visibility and show that root can
inspect the listed process FDs on this host. They do **not** establish that an
empty result means no host listener exists. A PID scan only enumerates the
reference types represented in process FD tables and has no atomic host-wide
snapshot. The updated checked-in probe includes a cross-UID Mach fileport
case; its privileged result is pending.

The current `macos.rs::correlate` also turns several `proc_pidinfo` failures
into an empty FD list. A production observer must distinguish a verified empty
holder list from an inspection failure before it can claim complete ownership.

## Socket behavior that a port probe must handle

A local bind experiment showed that, when both sockets use `SO_REUSEADDR`, a
wildcard `0.0.0.0` listener and a new exact `127.0.0.1` listener can coexist
on one port. A successful exact bind therefore cannot reject a wildcard
conflict. Omitting `SO_REUSEADDR` changes that result but would break the
contract's compatible `TIME_WAIT` restart behavior.

A separate IPv6 probe opened listeners with `IPV6_V6ONLY` both false and true.
Their `PROC_PIDFDSOCKETINFO` `insi_flags` differed by bit `0x8000`, showing a
bind-mode signal in the per-FD record on this host. The per-FD
`vinfo_stat.vst_uid` field was zero for a UID 501 socket, so it must not be
mistaken for the socket owner UID. The PCB socket metadata supplies a distinct
UID fact; FD holders and socket UID must not be conflated.

## Decisive negative-inventory counterexamples

The controlled
[`tests/macos/prove_fileport_listener.py`](tests/macos/prove_fileport_listener.py)
probe tested references outside ordinary process FDs. Both modes ran on this
host without sudo, using temporary loopback sockets.

| Case | State after original TCP FD closed | TCP connect | PCB record | Identity check |
| --- | --- | --- | --- | --- |
| Mach fileport | Zero TCP socket FDs; one socket fileport | Succeeded | Present | Fileport handle matched original |
| Queued Unix `SCM_RIGHTS` | No matching TCP FD or fileport | Succeeded | Present | FD received later had original handle |

The fileport mode used `fileport_makeport`, closed the TCP listener FD, then
confirmed that the fileport API named the same kernel socket. The queued-rights
mode sent the listener FD in an `AF_UNIX` message, closed the original FD, and
left the message unread while checking the process FD and fileport lists. The
listener still accepted a TCP connection. Receiving the message then restored
an FD for the same socket. The PCB stream showed the listener in both modes.

This falsifies the proposed privileged FD scan as a complete **negative**
listener inventory. Adding fileport enumeration covers the first case but not
the queued-rights case. Public XNU source describes how
[`fileport_makeport` retains a file reference](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_descrip.c)
and how [Unix rights retain one in a queued message](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/uipc_usrreq.c).
The local probes demonstrate these mechanisms on the installed host. The
public XNU release available for inspection is not the exact installed macOS
27 kernel, so source review alone must not substitute for host proof.

To reproduce the two local cases, run the probe normally and with
`--queued-rights`:

```sh
/usr/bin/python3 tests/macos/prove_fileport_listener.py
/usr/bin/python3 tests/macos/prove_fileport_listener.py --queued-rights
```

The first mode printed `socket_fd_count_after_close: 0`,
`fileport_listed: true`, `socket_identity_preserved: true`,
`pcb_correlated: true`, and `connected_after_fd_close: true`. The second
printed `no_listener_fd_while_queued: true`,
`no_fileport_while_queued: true`, `connected_while_queued: true`,
`pcb_correlated_while_queued: true`, and
`socket_identity_preserved_after_receive: true`. Python syntax checks and
`git diff --check` passed for the checked-in probes. These experiments do not
replace the macOS endpoint integration gate.

## Design consequence and proof gate

The accepted one-shot privileged observer remains a possible packaging and UX
shape: only a narrow read-only helper needs inspection privilege, while the
CLI and services stay under the invoking user. Its originally proposed macOS
data source has failed. A revised observer must take listener discovery from a
privileged **kernel TCP PCB inventory**, then correlate process FDs and
fileports for current holder evidence. The runtime must retain one PORT-1
meaning on Linux and macOS and reject unavailable, malformed, or incomplete
observations as `PORT_UNVERIFIABLE`. A helper protocol or `sudo -n` rule cannot
turn a partial data source into complete evidence.

The unresolved gate is a defensible completeness rule for a negative PCB
snapshot under the installed kernel's access policy. Root visibility of known
listeners, five matching generations, and a zero-error process scan are
positive tests only. The 49/47 root result shows why blindly requiring
`xinpgen` record-count equality does not work; removing the check without a
replacement proof is equally unsound. Process/FD/fileport correlation also
needs a precise treatment of a kernel-visible listener with no current
process handle, such as the queued-rights case.

Before a production cutover, establish that PCB rule and then prove exact and
wildcard IPv4/IPv6 conflicts across UIDs, both IPv6-only modes, shared and
transferred sockets, listener replacement and PID reuse, process-list growth
and inspection denial, socket loss, `TIME_WAIT` restart, and missing or
malformed observer responses. Run the same PORT-1 behavioral cases through
Linux and macOS endpoint integration. Independently verify helper ownership,
installation, upgrade, protocol, and authorization boundaries. The proof must
cover accepted ownership and rejected or unverifiable cases; a successful
host scan cannot stand in for it.

Until that gate is met, keep the current fail-closed behavior. Do not remove
the discrepancy check merely to pass macOS CI, treat a successful bind or
TCP/UDP probe as ownership, substitute `nettop`, or skip endpoint tests.
Endpoint-less services make no PORT-1 addressability claim. The failing
endpoint portion of `nix run .#ci -- --dirty` remains a merge blocker for this
platform. The related authority and verification routes are
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md#ports-state-containment) and
[`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md#local-versus-hosted-ci).
