# PORT-1 privileged observer design review

Status: feasibility review, 29 September 2026. This document does not change
the shipped endpoint contract or enable endpoint starts on hosts where the
current observer returns `PORT_UNVERIFIABLE`.

## Decision boundary

PORT-1 has one meaning on Linux and macOS: before service preparation, a
complete host listener observation must exclude exact and wildcard conflicts;
before readiness commits, it must establish exact socket and contained process
ownership. A successful bind or connection is insufficient. The runtime owns
these decisions and their error precedence. The observer supplies OS facts or
an explicit incomplete result; it cannot decide service lifecycle outcomes.

The candidate is a single, stateless, read-only observer executable with the
same request and response protocol on both platforms. It is invoked on demand
with inspection privilege. It never executes a service, accepts a manifest,
opens a state directory, writes a registry, signals a process, or invokes Nix.
The runtime validates its protocol version, request scope, and complete result
before it uses any returned record. An unavailable, incompatible, failed, or
incomplete observer produces `PORT_UNVERIFIABLE` before prepare or ready commit.
There is no native fallback and no OS-version-specific weakening of PORT-1.

## Privilege and installation

Only the observer needs privilege to inspect other processes' sockets and FDs.
The CLI, service processes, probes, and state operations continue under the
invoking user. A one-time system installation must place the observer in a
root-owned, non-writable location and authorize only that executable's fixed
observation command for the invoking user. The proposed first implementation
uses a narrowly scoped `sudo -n` rule and a separate observer binary, not a
privileged entry point in the general runtime. An install or upgrade must
validate the installed object and rule; missing authorization fails closed
without prompting during a run. This avoids a required long-lived daemon, but
the installer and upgrade protocol require their own security review and tests.

The observer must treat its caller and input as untrusted. It accepts only a
bounded request for loopback TCP endpoints in the caller's network scope and
returns only matching listener evidence. Standard input, environment, inherited
FDs, paths, and working directory cannot influence which privileged files or
programs it accesses. It must not expose unrelated host socket inventory.

## Observation protocol and implementation

The protocol carries a version, requested address/port tuples, and bounded
response records with address, family, port, kernel socket identity, socket
owner UID, IPv6-only bind mode, and every observed FD holder's PID, process
group, and start identity. A response has an explicit complete/incomplete
outcome; errors never masquerade as an empty record set. The runtime performs
the existing conflict, containment, stabilization, and lifecycle decisions.

On macOS, the candidate primary source is privileged `libproc` process/FD
enumeration and `PROC_PIDFDSOCKETINFO`, including TCP state and IPv6 flags.
`pcblist_n` may be used as a cross-check, but its `xinpgen` count cannot be the
sole completeness certificate: public XNU code can skip records, and a local
root probe observed 49 advertised records with 47 decoded records while both
controlled listeners remained visible. Every live process/FD inspection error
must be classified and cause an incomplete result. Process and socket identity
must be checked across repeated observations to bound replacement races.

On Linux, the observer should use the existing `SOCK_DIAG` socket identities
and process-FD correlation under the same protocol. It must likewise reject
unreadable live process/FD evidence. Platform mechanisms may differ; the
runtime-visible facts and failure meanings must not.

## Feasibility evidence and remaining proof

The local macOS 27.0.1 unprivileged probe repeatedly decoded only its own
listener despite a stable child listener. Running Apple's `sysctl` under `sudo`
showed both listeners in five snapshots, with matching kernel socket and child
FD identities. A separate root probe dropped the child to UID 501; it again
matched that child's socket to its FD. One privileged process scan inspected
all 1,239 listed PIDs, found 687 socket FDs, and reported zero live inspection
failures. A local IPv6 probe found the `IPV6_V6ONLY` distinction in the
per-FD `insi_flags` field. These are positive feasibility observations, not a
host-wide completeness or release proof.

The read-only, manually run probe is
[`tests/macos/prove_observer_feasibility.py`](../tests/macos/prove_observer_feasibility.py).
Run it normally for the baseline. `sudo -v` followed by its `--sudo-sysctl`
mode elevates only Apple's `sysctl` command. Its `--as-root` mode tests
cross-UID FD correlation and process inspection; review the script before
running that mode. It binds only temporary loopback sockets.

Before implementation is accepted, prove exact and wildcard IPv4/IPv6
conflicts across users; both IPv6-only modes; shared/transferred FDs; listener
replacement and PID reuse; process-list growth and inspection denial; socket
loss; `TIME_WAIT` restart; and missing/malformed observer results. Run the same
behavioral cases through the Linux and macOS endpoint integration suites.
Independently verify the installed helper's ownership, upgrade, protocol, and
authorization boundaries. A root-visible known child and one error-free PID
scan do not establish that every host listener is observable on every host.

If privileged per-FD enumeration cannot support a complete negative snapshot,
this design must stop at the feasibility gate. Removing the discrepancy check
from the existing `pcblist_n` observer would not establish completeness;
neither would accepting an empty sample or substituting `nettop`.
