# Implementation language simplification plan

Status: source cutovers and Linux verification complete; native macOS
verification deferred to the user's host. Baseline: `58611ee`.

Completed on aarch64 Linux:

- Polyglot removal (`c579903`): retained manifests, derivation vectors, and
  publication evaluation passed.
- Static HTTP synthetic/downstream helpers and endpoint-less worker (`af54ec7`): affected
  manifests built; the complete runtime gate passed, including repeat lifecycle,
  protocol checks and two-slot isolation. macOS execution remains pending.
- Reth protocol tool composition (`ec0225e`, `b36f100`): nine Rust composition
  tests pass, including real Reth authentication and malformed secret bytes
  rejected before networking. Custom Python framing and harnesses are deleted.
- Reference checks (`e84add1`): retained independent Nix vectors and packaged
  topic-output comparisons pass; the duplicate Python selector is deleted.
- Nix-packaged runtime fixtures and shared CI floor (`5c24e14`): 164 focused
  integration tests and three library tests pass. Both hosted platforms now run
  the complete fixture-backed floor, including the Darwin decoder target.
- Private Rust upgrade helper (`73c06d5`): initial unit/integration tests,
  formatting, Clippy, packaged syntax and source-selection checks pass. The
  source editor and guarded file mutation replace shell parsing, Python and C;
  concurrency limitations are documented in the contract and user guide.
  Follow-up subtraction retains five source-edit/concurrent-edit tests and
  removes eight redundant or low-value tests, including the duplicate SIGTERM
  harness. The public gate owns actual interruption, apply/no-op, and stale
  candidate coverage. The reduced suite, formatting, Clippy, and packaged
  upgrade build pass on aarch64 Linux.
- SDK-derived Rust macOS decoder (`c7a113f`): production build script, decoder
  and tests cross-typecheck against actual SDK 14.4, with generated layout
  assertions and Clippy. Linux all-target checking and Darwin package evaluation
  pass. This does not prove native macOS execution.
- Linux release builds pass for `nixfied-cli`, `install`, and `nixfied-runtime`.
- Full `nix run .#ci -- --dirty` passes on aarch64 Linux: flake checks,
  fixture-backed Cargo floor, all 20 runtime gate cases, reference and source
  isolation checks, upgrade transactions/source selection/historical reports,
  and downstream adoption. The final malformed-secret-byte fix was separately
  rebuilt and passed all nine Reth tests after the CI wrapper was realised.
- Active source inventory contains no standalone Python, Perl, C, JavaScript,
  TypeScript, or shell files. Remaining command expressions, SQL, and
  Nix-packaged shell glue are intentional; frozen archives are unchanged.

### Native macOS handoff

The user will run these on a macOS host after the Linux work is complete:

```sh
nix run .#ci -- --dirty
nix build .#nixfied-cli .#install .#nixfied-runtime --no-link
```

The full CI floor includes the decoder, endpoint, service, and output tests;
the gates cover static-server lifecycle and upgrade mutation on Darwin. Keep
this checklist until those results are recorded. SDK cross-compilation proves
types and layouts, not live socket observation or Darwin filesystem behavior.

Reduce repository-owned implementations to Nix and Rust while preserving useful
behavior and independent proofs. Delete redundant work first, use existing tools
through Nix-packaged shell next, and introduce small Rust components only for
substantive behavior that tools cannot express simply and safely.

This is an execution checklist, not a new contract. [AGENTS.md](../AGENTS.md),
[CONTRACT.md](CONTRACT.md), and [DEVELOPMENT.md](DEVELOPMENT.md) remain authoritative.
Update this plan with decisions and check results during delivery; remove it when
complete after moving lasting explanations into their owning documentation.

## Scope and invariants

- Keep bounded jq, sed, and awk expressions, SQL, declarative configuration,
  external dependencies, and frozen historical upgrade archives.
- Keep Nix responsible for declarations, packaging, and build-time validation;
  keep runtime execution, endpoint ownership, containment, and cleanup in Rust.
  Adapter commands may perform their own application protocol checks.
- Every deleted test must name a remaining independent proof or a removed
  requirement. Do not translate tests of a deleted implementation mechanically.
- Each cutover removes its superseded implementation and wiring together.
  Intermediate commits may leave other planned areas untouched, but must not
  introduce dual implementations or reduce existing behavior.
- Preserve exact public output, errors, rejection phases, and manifest semantics
  unless an explicit contract change is included with all coupled surfaces.
  No runtime ABI change is intended merely to replace child implementations.
- Keep shell programs embedded and packaged in Nix. A packaged interpreter alone
  does not make shell source authored in Rust a Nix-packaged program.

## Current baseline additions

The architecture review preceded two changes that this work must preserve:

- Reth now has `peer` mode in `nix/adapters/reth-probe.nix`: curl/jq obtain a
  validated public identity, and native Reth performs the RLPx handshake against
  the planned endpoint. Reuse that packaging and preserve the independent
  `nix/checks/reth-peer-probe.rs` suite. Do not reimplement RLPx.
- Upgrade now distinguishes non-evaluating plan, checked apply, and forced apply.
  It preserves complete selected source references and supports moving versus
  commit-pinned inputs. Preserve current UPGRADE-1 and the source-selection tests;
  the earlier assumption that plan evaluates the candidate is obsolete.

## Ordered implementation commits

Suggested subjects below are lower case. Each commit includes its relevant
documentation and tests, and must leave a coherent working design.

### 1 Remove the redundant polyglot example

Suggested subject: `remove redundant polyglot example`.

Delete `examples/polyglot-stack`, its manifest/package and environment wiring in
`flake.nix`, its runtime-gate task and composite step, and its README entry.
Update any publication references found by a repository-wide caller search.

The invariant is continued coverage of multi-service composition, per-task
requirements, DAG execution, and cleanup. Downstream, composite, and minimal
examples own that remaining evidence; language variety itself is not a runtime
requirement. Nix publication and compiler checks must reject stale references.
Prove removal with publication checks and the retained example gates.

### 2 Replace demonstration servers with packaged tools

Suggested subject: `replace demonstration python helpers with packaged tools`.

First exercise the pinned static-server candidate, `darkhttpd`, with a Nix-built
response directory and curl. Verify foreground execution, exact loopback bind,
assigned port, meaningful response checking, termination, and repeat startup.
Package metadata advertises Linux and macOS support; execution is not yet proved.

Replace Python in `nix/adapters/synthetic.nix` and
`examples/downstream/nixfied.nix`. Preserve task output and declarations. The
downstream worker must check the API before starting; failed upstream or invalid
responses reject before successful startup/task completion. Use existing Postgres
protocol tooling rather than raw ping bytes. Share a small Nix packaging function
only if the final implementations demonstrate the same repeated responsibility.
Use a small Rust server only if the existing server fails the required behavior.

Simplify the endpoint-less worker in `examples/toolchain/nixfied.nix`: perform a
real Postgres check, publish readiness only after success, and use a foreground
long-running command. Remove stale readiness before startup work. Do not claim
marker existence proves heartbeat freshness. Correct the ineffective placeholder
check and misleading comments rather than preserving them.

Nix owns executable packaging and graph declarations; child tools own protocol
success. Preserve source substitution, dependency ordering, lifecycle, slot
isolation, and cleanup proofs. Build affected manifests and run their gates,
including repeated starts and two slots. Verify the server behavior on macOS too.

### 3 Replace custom Reth protocols with tool composition

Suggested subject: `replace custom reth probe protocols with packaged tools`.

Use the existing `nix/adapters/reth-probe.nix` entry point for all modes. HTTP
uses curl plus bounded JSON-RPC validation. Test packaged websocat for one complete
WebSocket request/response. Test a packaged JWT signer with the actual hex-encoded
32-byte state-file key; token and key must not appear in argv, diagnostics, or
unnecessary persistent files. A small Rust signer is the fallback if existing
signer tools cannot satisfy these conditions simply. Do not implement framing or
cryptography in shell. Preserve the native peer mode and its current tests.

Reject invalid arguments before networking and unsuccessful transport, HTTP,
JSON-RPC, authentication, or result validation before exit zero. Preserve direct
connections, bounded input, safe fixed diagnostics, and pipeline failure status;
disable unwanted proxy/config discovery and redirects. Runtime policies retain
deadlines and retry ownership. Explicitly test any validation differences caused
by changing JSON parsers instead of silently broadening accepted input.

Retain independent controlled-peer tests of composition and secret handling,
using the existing Rust test packaging where cohesive. Delete custom WebSocket
framing tests only when upstream tools own that implementation. Move real-Reth
missing/wrong-credential checks into the existing Reth gate, then delete the
standalone Python integration harness and duplicate lifecycle machinery. Keep
the adopter-facing smoke task. Remove all three Python files and their test/app
wiring in this cutover, after the retained proofs pass.

### 4 Simplify reference tests and package shell fixtures

Suggested subjects: `simplify reference checks without python` and
`package shell test fixtures in nix`.

In `nix/checks/reference.nix`, delete the Python Markdown-selector reconstruction:
literal Nix fence and composition vectors already own the independent algorithm
proof. Preserve authored topic inclusion/exclusion, prose, ordering, backlinks,
cross-document composition, and actual CLI-output checks using Nix assertions
and bounded jq/grep/diff commands. Keep existing AWK formatting and jq dispatch.
Run reference/freshness checks and dirty downstream/source-isolation gates.

Replace Rust-inline trivial exit/marker programs with existing test-child modes.
Admission fixtures that test only executable metadata can contain inert bytes.
Package necessary probe, endpoint-race, and poisoned-PATH shell programs in Nix,
passing temporary values as arguments or environment. Preserve rejection before
effects, whole-round replacement races, and the no-Nix-in-runtime proof. Keep
waits bounded; do not introduce a general fixture scripting framework.

Move hosted CI command bodies into Nix-packaged check commands where required.
Keep the independent Cargo floor and explicit macOS coverage. Include the
currently omitted `macos_decoder` target. Run affected admission, launch,
service, endpoint, state, and fixture tests through their Nix environment.

### 5 Consolidate upgrade source editing and guarded mutation

Suggested subject: `consolidate upgrade file mutation in rust`.

Keep Nix resolution, source selection, evaluation, documentation comparison, and
reporting as Nix-packaged tool orchestration. Replace the shell source-rewriter,
embedded Python replacement, C exchange executable, and coupled apply/rollback
state with one private Rust owner. Preserve package isolation; do not add these
dependencies to the generic runtime or dependency-free installer by convenience.

Before coding, specify the supported source-edit shapes and concurrency model.
Reject ambiguous edits before effects. Represent expected originals, candidates,
conflicts, apply failures, and rollback failures explicitly. Preserve unrelated
source bytes and project-owned declarations. A per-file exchange is not an atomic
two-file update or universal compare-and-swap; do not promise protection from
arbitrary concurrent editors, SIGKILL, or power loss without a proved mechanism.
Account for the existing post-exchange restoration race in that design.

Clarify UPGRADE-1's Nix-only/Rust wording alongside the private-helper cutover;
retain the public flake-app surface, current modes, outputs, and exit statuses.
Keep source-selection, syntax, no-op, force/plan precedence, stale-candidate,
interruption, rollback, and exact historical report proofs. Add focused filesystem
fault-window and source-edit tests. Never rewrite historical archives for this
cleanup. Verify platform-specific mutation on both Linux and macOS.

### 6 Replace the macOS C decoder with SDK-derived Rust bindings

Suggested subject: `decode macos socket records through rust sdk bindings`.

First prove binding generation and layouts against the selected macOS SDK and
supported target. Move decoding into the existing Rust endpoint module, with
SDK-derived types rather than copied offsets. Delete the C decoder, C fixture,
and their build wiring together only after equivalent tests pass.

The invariant is complete, exact managed-listener evidence including socket
handle and generation. The Rust OS boundary rejects short, malformed, denied,
and unsupported records rather than treating them as empty or successful scans.
Preserve injected IPv4/IPv6, short/zero-return, permission/churn, invalid-family,
zero-handle, and non-listening cases, plus live endpoint replacement tests.
Require macOS decoder, endpoint, service, output, and release-build evidence.
Linux success alone cannot complete this step. If binding feasibility fails,
record the unresolved boundary rather than weakening the observer to remove C.

## Verification and completion

Recheck status and current authority before each step. Record retained/deleted
proofs, checks run, and unresolved platform coverage with each completed step.
Use focused checks first, then the fixture-backed floor and full dirty gate:

```sh
nix run .#test
nix run .#ci -- --dirty
nix build .#nixfied-cli .#install .#nixfied-runtime --no-link
```

The current CI wrapper forwards `--dirty` to the final gate. Ensure new source
files are visible to Nix's Git source selection. Do not substitute raw Cargo for
the fixture-backed floor or repeat already-passing full checks without cause.

Finish with an inventory of active standalone and embedded implementation code,
including Rust-created scripts and Nix-emitted programs. Historical archives and
external package source are excluded. No active Python, Perl, or C implementation
should remain; SQL and bounded command expressions remain intentional. Any
unfinished SDK/tool feasibility or platform verification means the corresponding
step remains incomplete. Introduce no brittle keyword-based language scanner as
a substitute for this review.
