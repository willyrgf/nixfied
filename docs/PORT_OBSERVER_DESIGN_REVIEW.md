# PORT-1 privileged observer design review

Status: feasibility review blocked by negative-inventory counterexamples,
29 September 2026. This document does not change the shipped endpoint
contract or enable endpoint starts on hosts where the current observer returns
`PORT_UNVERIFIABLE`.

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

The originally proposed macOS primary source, privileged `libproc` process/FD
enumeration, **fails the negative-inventory proof**. A TCP listener can remain
live after its last ordinary FD closes. A Mach fileport can retain it, and a
queued Unix `SCM_RIGHTS` message can retain it even when neither an FD nor a
fileport names it. Both cases were reproduced locally; the listener accepted a
connection and retained its kernel socket identity. Adding fileport inspection
does not repair the queued-rights gap. Process-handle enumeration therefore
cannot certify the absence of conflicting listeners.

A revised macOS candidate must use a privileged kernel TCP PCB inventory as
the authority for listener discovery, then correlate process FDs and fileports
for ownership evidence. Its `xinpgen` count cannot be the sole completeness
certificate: public XNU code can skip records, and a local root probe observed
49 advertised records with 47 decoded records while both controlled listeners
remained visible. The installed macOS 27 kernel source is not public at the
same version, and the current evidence does not establish an authoritative
completeness rule for negative PCB snapshots. Every live process inspection
error must still produce an incomplete result. Repeated identity checks must
bound replacement races; they cannot make a process scan atomic.

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

The separate controlled
[`prove_fileport_listener.py`](../tests/macos/prove_fileport_listener.py)
proved two structural gaps in an FD-based inventory. With a Mach fileport, its
ordinary socket FD count became zero, the fileport API named the unchanged
socket, and both the TCP connection and PCB correlation succeeded. With a
queued `SCM_RIGHTS` message, neither the process FD list nor the fileport list
named the TCP listener, yet a connection and PCB correlation succeeded; after
receiving the message, the recovered FD had the original socket identity.
These tests run without sudo because the observer and socket owner are the same
user. Public XNU source describes the separate
[`fileport` reference](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_descrip.c)
and [queued Unix rights](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/uipc_usrreq.c)
mechanisms. The tests demonstrate them on the installed host.

The read-only, manually run probe is
[`tests/macos/prove_observer_feasibility.py`](../tests/macos/prove_observer_feasibility.py).
Run it normally for the baseline. `sudo -v` followed by its `--sudo-sysctl`
mode elevates only Apple's `sysctl` command. Its `--as-root` mode tests
cross-UID FD and fileport correlation and process inspection; review the script
before running that mode. It binds only temporary loopback sockets.

Before implementation is accepted, establish a kernel inventory completeness
rule that survives the installed macOS access policy, or stop the design at
this gate. Prove exact and wildcard IPv4/IPv6
conflicts across users; both IPv6-only modes; shared/transferred FDs; listener
replacement and PID reuse; process-list growth and inspection denial; socket
loss; `TIME_WAIT` restart; and missing/malformed observer results. Run the same
behavioral cases through the Linux and macOS endpoint integration suites.
Independently verify the installed helper's ownership, upgrade, protocol, and
authorization boundaries. A root-visible known child and one error-free PID
scan do not establish that every host listener is observable on every host.

Privileged per-FD enumeration has now failed the negative snapshot gate.
Removing the discrepancy check from the existing `pcblist_n` observer would
not establish completeness; neither would accepting an empty sample or
substituting `nettop`. No production cutover is justified by the present proof.
