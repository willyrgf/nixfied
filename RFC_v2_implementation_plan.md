# RFC v2 Milestone 0 Implementation Plan

Source of truth: `RFC_v2.md`.

This plan is for Milestone 0 only. It must not redesign the framework, preserve v1 behavior, add compatibility layers, or widen scope beyond the walking skeleton selected in the RFC.

## M0 Boundary

Milestone 0 proves one vertical spine:

```text
typed Nix module
  -> Nix-store model.json
  -> Nix-free runtime admission
  -> SQLite registry
  -> foreground synthetic service
  -> readiness with endpoint ownership
  -> dependent task
  -> down / ps / clean
```

M0 explicitly does not implement:

- workflows;
- adapters;
- service reuse;
- real secret injection;
- multi-slot execution;
- SQLite migrations;
- runtime adapter protocols;
- manifest envelopes;
- crash-hardening GC beyond marker-gated refusal rules;
- historical v1 commands, layouts, fixtures, sidecars, APIs, or behavior.

## Agent Ownership

| Agent | M0 ownership |
| --- | --- |
| Model/Nix Agent | Minimal M0 `model.json` schema, typed Nix module shape, compiler output, closure metadata, generated minimal views, and `PREPARE-1` compile/prepare behavior. |
| Runtime Core Agent | Rust model loading, serde type boundaries, ABI/toolchain checks, store-origin admission, `computedModelHash`, closure verification, target/source checks, and typed errors. |
| Process/Service Agent | `ExecSpec`, `EndpointSpec`, `ProbeSpec`, `ServiceSpec`, `TaskSpec`, `LifecycleOpSpec`, foreground service spawning, process group ownership, readiness, dependent task execution, and shutdown. |
| Registry/State Agent | SQLite registry schema v1, ordered events, run/service/process records, state root derivation, `.nixfied-state.json`, summary/log roots, `ps` reconciliation data, and marker-gated cleanup. |
| Verification Agent | Acceptance tests, negative tests, platform assumptions, port ownership proof, runtime-without-Nix proof, and the downstream-shaped minimal example. |

## Repository Structure To Create

The current repository is greenfield for implementation. Create the RFC layout directly.

```text
flake.nix
flake.lock

nix/
  modules/
    default.nix
    project.nix
    primitives.nix
    source.nix
    state.nix
  compiler/
    default.nix
    resolve.nix
    validate.nix
    derive.nix
    emit-model.nix
    views.nix
  spec/
    constants.nix
    model-v1.nix
  lib/
    closures.nix
    identifiers.nix
    json.nix
    target.nix

runtime/
  Cargo.toml
  Cargo.lock
  crates/
    nixfied-model/
      Cargo.toml
      src/
        lib.rs
        constants.rs
        types.rs
        validation.rs
        error.rs
      tests/
        m0_model_contract.rs
        fixtures/
    nixfied-runtime/
      Cargo.toml
      src/
        main.rs
        lib.rs
        error.rs
        model_loader.rs
        admission/
          mod.rs
          origin.rs
          abi.rs
          target.rs
          source.rs
          closures.rs
          secrets.rs
        placement.rs
        registry/
          mod.rs
          schema.rs
          events.rs
          records.rs
          sqlite.rs
        state/
          mod.rs
          marker.rs
          cleanup.rs
        process/
          mod.rs
          identity.rs
          reconcile.rs
          spawn.rs
        endpoint/
          mod.rs
          owner.rs
        probe.rs
        service.rs
        task.rs
        summary.rs
        logs.rs
      tests/
        m0_admission.rs
        m0_registry.rs
        m0_ports.rs
        m0_service_task.rs
        m0_cleanup.rs
        m0_no_nix.rs
    nixfied-cli/
      Cargo.toml
      src/
        main.rs
        compile.rs
        views.rs
      tests/
        m0_views.rs

examples/
  m0-minimal/
    flake.nix
    nixfied.nix
    src/
      input.txt

tests/
  m0/
    prove-downstream-minimal.sh
    prove-runtime-without-nix.sh
```

## Minimal M0 Model Schema

`model.json` remains the only semantic seam. There is no required `manifest.json`, `schema.json`, or `capabilities.json` authority.

Required top-level fields:

```json
{
  "modelVersion": 1,
  "toolchainId": "nixfied-toolchain:m0:1",
  "runtimeAbi": "nixfied-runtime-abi:m0:1",
  "generator": {},
  "project": {},
  "target": {},
  "codebases": [],
  "environments": {},
  "slotPolicy": {},
  "capabilities": {},
  "runtimeConstraints": {},
  "surfaces": [],
  "placement": {},
  "state": {},
  "secrets": [],
  "closures": [],
  "execs": {},
  "services": {},
  "tasks": {},
  "workflows": {},
  "docs": {}
}
```

M0 field constraints:

| Field | M0 shape |
| --- | --- |
| `modelVersion` | Exact integer `1`. |
| `toolchainId` | Exact string `nixfied-toolchain:m0:1`. |
| `runtimeAbi` | Exact string `nixfied-runtime-abi:m0:1`. |
| `generator` | Present and recorded for provenance; exact admission equality is not driven by this field. |
| `project` | Stable `projectId` and name. |
| `target` | Nix `system`, OS family, architecture, closure system, and required runtime capabilities. |
| `codebases` | Exactly one `live-workspace` codebase, `codebaseId = "main"`. |
| `environments` | Exactly one environment, `dev`. |
| `slotPolicy` | `min = 0`, `default = 0`, `max = 0`. |
| `placement` | Logical templates and candidate port window only; no host-absolute paths. |
| `state` | Marker identity, state epoch, cleanup policy, persistence policy. |
| `secrets` | Empty only. Non-empty descriptors are rejected in M0. |
| `closures` | Already-realised store paths for runtime-dispatched executables/helpers. |
| `execs` | Minimal synthetic helper exec, with lifecycle/task bindings providing typed operation inputs. |
| `services` | One foreground synthetic service. |
| `tasks` | One bounded dependent task. |
| `workflows` | Empty only. Workflow execution is unsupported in M0. |
| `capabilities`, `surfaces`, `docs` | Minimal discoverability fields generated from and carried in the model. |

`model.json` must not embed a self-hash. Runtime computes:

```text
computedModelHash = sha256(raw model.json bytes)
```

The runtime records this hash in registry events, summaries, logs, and error payloads when raw bytes were readable.

## Minimal Primitive Shapes

### `ClosureSpec`

```json
{
  "closureId": "m0-helper",
  "kind": "executable",
  "storePath": "/nix/store/...",
  "executable": "/nix/store/.../bin/...",
  "targetSystem": "aarch64-darwin",
  "operationBindings": [
    "service.synthetic.start",
    "service.synthetic.stop",
    "task.smoke.run"
  ],
  "requiresExecutable": true,
  "effects": [
    "process",
    "network-listener"
  ]
}
```

M0 verification:

- `storePath` and `executable` are under the Nix store;
- referenced closure is declared in `closures`;
- executable path exists;
- executable bit is set when `requiresExecutable = true`;
- `targetSystem` matches `target.closureSystem`;
- operation binding exists in model primitives.

NAR/content metadata stays optional in M0.

### `ExecSpec`

Required fields:

- `execId`;
- `closureId`;
- `executable`;
- `args`;
- `env`;
- `codebaseId`;
- `cwd`;
- `stdin`;
- `timeoutMs`;
- `outputCapture`;
- `cancellationMode`.

M0 supports only declared source observation through `codebaseId = "main"`.

### `EndpointSpec`

Required fields:

- `endpointId`;
- `protocol = "tcp"`;
- `host = "127.0.0.1"`;
- fixed or candidate-window port policy;
- `ownershipVerification = "required"`;
- socket activation disabled.

### `ProbeSpec`

Required fields:

- `probeId`;
- TCP connect or HTTP GET probe;
- timeout;
- retry interval;
- max attempts.

The probe alone never proves readiness; endpoint ownership must also be verified.

### `LifecycleOpSpec`

M0 supports only:

- `Start`;
- `Ready`;
- `Stop`.

Each lifecycle operation binds to generic primitives and has typed terminal success/failure semantics.

### `ServiceSpec`

Required fields:

- `serviceId = "synthetic"`;
- foreground execution only;
- lifecycle op bindings for `Start`, `Ready`, and `Stop`;
- one endpoint set;
- readiness policy;
- stop policy;
- state/log references;
- containment requirement `process-group`;
- no reuse policy beyond run-scoped ownership.

### `TaskSpec`

Required fields:

- `taskId = "smoke"`;
- bounded exec;
- depends on `synthetic` readiness;
- exit policy;
- stdout/stderr capture;
- artifact/log/summary references.

## Typed Nix Module Shape

Expose a downstream-importable module under `nix/modules/default.nix`.

Initial options:

- `nixfied.project.projectId`;
- `nixfied.project.name`;
- `nixfied.target.system`;
- `nixfied.codebases.main`;
- `nixfied.environments.dev`;
- `nixfied.services.synthetic`;
- `nixfied.tasks.smoke`;
- `nixfied.state`;
- `nixfied.secrets`, default `[]`, assertion rejects non-empty values;
- `nixfied.workflows`, default `{}`, assertion rejects non-empty values;
- `nixfied.slotPolicy`, fixed to slot `0`.

Compiler pipeline:

```text
resolve -> validate -> derive -> emit model
```

Nix validates:

- invalid names;
- broken references;
- unsupported slots;
- non-empty secrets;
- non-empty workflows;
- malformed primitive declarations;
- missing closure derivations;
- invalid source/target policy;
- host-absolute placement accidentally entering model output.

## Compile And Prepare Behavior

`nixfied compile` evaluates Nix, validates typed options, realises all runtime closures, and emits one Nix-store output:

```text
/nix/store/...-nixfied-model/
  model.json
  views/
    schema.json
    docs.md
    capabilities.json
```

Rules:

- `model.json` is the only required semantic artifact.
- Generated views are disposable projections.
- No manifest is emitted or required.
- `prepare` is either an explicit CLI alias or the final compile phase.
- `PREPARE-1` is enforced by making runtime closures derivation inputs so they are realised before runtime starts.
- `nixfied-runtime` never invokes Nix or `nix-store`.
- `--allow-non-store-model` exists only as an unstable framework-development/test escape hatch.

## Runtime Core Boundaries

`nixfied-model` owns all serde types. `nixfied-runtime` must not deserialize into ad hoc model structs.

Minimal Rust type boundary:

```text
Model
Generator
Project
Target
RuntimeCapabilities
Codebase
SourcePolicy
Environment
SlotPolicy
RuntimeConstraints
Placement
StatePolicy
ClosureSpec
ExecSpec
EndpointSpec
ProbeSpec
LifecycleOpSpec
ServiceSpec
TaskSpec
SecretRef
```

Serde policy:

- deny unknown fields;
- require exact fields;
- avoid compatibility defaults except accepted M0 constants;
- keep host-materialized paths out of model structs;
- do not store runtime state in model structs.

Model loading:

1. Accept a model path.
2. Read raw bytes once.
3. Compute `computedModelHash = sha256(raw bytes)`.
4. Deserialize through `nixfied-model`.
5. Return `LoadedModel { path, raw_len, computed_model_hash, model }`.

Never reserialize to hash.

Admission order before any process starts:

1. Store-origin check.
2. Exact `modelVersion`, `runtimeAbi`, and `toolchainId` check.
3. Target and runtime capability check.
4. Source policy check for the one live workspace placeholder.
5. Closure declaration, existence, executability, and target compatibility check.
6. Secret policy check: empty only.
7. State policy shape and writable placement check.
8. Registry acquisition.

## SQLite Registry Schema V1

One SQLite WAL database per `(projectId, env, slot)`:

```text
<state_root>/registry/registry.sqlite3
```

M0 assumes local filesystems with SQLite WAL and lock semantics supported by SQLite. Network filesystems and remote coordination are unsupported.

Schema proposal:

```sql
PRAGMA journal_mode = WAL;
PRAGMA user_version = 1;

CREATE TABLE registry_meta (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  project_id TEXT NOT NULL,
  environment TEXT NOT NULL CHECK (environment = 'dev'),
  slot INTEGER NOT NULL CHECK (slot = 0),
  runtime_abi TEXT NOT NULL,
  toolchain_id TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE events (
  seq INTEGER PRIMARY KEY,
  at TEXT NOT NULL,
  event_type TEXT NOT NULL,
  run_id TEXT,
  service_instance_id TEXT,
  process_key TEXT,
  computed_model_hash TEXT,
  payload_json TEXT NOT NULL
);

CREATE TABLE runs (
  run_id TEXT PRIMARY KEY,
  status TEXT NOT NULL,
  model_path TEXT NOT NULL,
  computed_model_hash TEXT NOT NULL,
  runtime_abi TEXT NOT NULL,
  toolchain_id TEXT NOT NULL,
  generator_json TEXT NOT NULL,
  target_json TEXT NOT NULL,
  source_json TEXT NOT NULL,
  summary_path TEXT
);

CREATE TABLE services (
  service_instance_id TEXT PRIMARY KEY,
  service_name TEXT NOT NULL,
  service_address_hash TEXT NOT NULL,
  endpoint_identity_hash TEXT NOT NULL,
  state_identity_hash TEXT NOT NULL,
  runtime_compatibility_hash TEXT NOT NULL,
  target_identity_hash TEXT NOT NULL,
  status TEXT NOT NULL,
  endpoint_json TEXT NOT NULL,
  state_root TEXT NOT NULL
);

CREATE TABLE processes (
  process_key TEXT PRIMARY KEY,
  pid INTEGER NOT NULL,
  pgid INTEGER NOT NULL,
  start_identity TEXT NOT NULL,
  command_json TEXT NOT NULL,
  run_id TEXT NOT NULL,
  service_instance_id TEXT,
  status TEXT NOT NULL
);

CREATE TABLE ports (
  endpoint_key TEXT PRIMARY KEY,
  service_instance_id TEXT NOT NULL,
  address TEXT NOT NULL,
  port INTEGER NOT NULL,
  status TEXT NOT NULL,
  owner_process_key TEXT
);

CREATE TABLE run_leases (
  run_id TEXT PRIMARY KEY,
  owner_token TEXT NOT NULL,
  heartbeat_at TEXT NOT NULL,
  expires_at TEXT NOT NULL
);

CREATE TABLE cleanups (
  cleanup_id TEXT PRIMARY KEY,
  target_path TEXT NOT NULL,
  marker_json TEXT,
  status TEXT NOT NULL,
  refusal_reason TEXT
);
```

Every state mutation writes an `events` row in the same transaction. `events.seq` is the total per-slot ordering source. Wall-clock timestamps are diagnostic only.

## State Roots And Marker Schema

Runtime derives host paths at admission:

```text
state_base = $NIXFIED_STATE_DIR || platform default
state_root = <state_base>/<projectId>/dev/0
registry_dir = <state_root>/registry
run_dir = <state_root>/runs/<runId>
logs_dir = <run_dir>/logs
artifacts_dir = <run_dir>/artifacts
summary_path = <run_dir>/summary.json
```

Every runtime-owned state root contains `.nixfied-state.json`.

M0 marker proposal:

```json
{
  "markerVersion": 1,
  "projectId": "example",
  "environment": "dev",
  "slot": 0,
  "stateKind": "slot",
  "serviceInstanceId": null,
  "stateEpoch": "m0",
  "cleanupPolicy": "delete-on-clean",
  "modelPath": "/nix/store/.../model.json",
  "computedModelHash": "...",
  "runtimeAbi": "nixfied-runtime-abi:m0:1",
  "toolchainId": "nixfied-toolchain:m0:1",
  "target": {}
}
```

Cleanup gates:

- canonicalize state base and target path before deletion;
- refuse path escape;
- refuse symlink traversal;
- refuse unmarked roots;
- refuse marker identity mismatch;
- refuse active leases;
- refuse live process references;
- refuse active port reservations;
- refuse protected persistent state because explicit purge is deferred;
- record cleanup intent and terminal cleanup event transactionally.

## Process And Service Flow

1. Admit the model.
2. Materialize placement and open registry.
3. Write admission and run events.
4. Reserve endpoint intent transactionally.
5. Spawn the synthetic service as a foreground child in a runtime-owned process group.
6. Record pid, pgid, platform start identity, command metadata, owning run, and service instance before readiness.
7. Run readiness probe.
8. Verify endpoint ownership against tracked process identity.
9. Mark service ready.
10. Run the dependent task.
11. Capture task stdout/stderr, exit status, logs, artifacts, and summary.
12. On `down`, signal the whole process group, wait, and escalate according to M0 stop policy.
13. On `ps`, reconcile registry process records against OS process state before reporting.
14. On `clean`, apply marker-gated cleanup rules.

Foreground-only service behavior is required. Daemonization, double-fork, `setsid` escape, or lack of stable handoff fails as `PROC_ESCAPE`.

## Port Ownership Verification Strategy

M0 supports TCP loopback endpoints only.

Readiness requires two facts:

1. the endpoint responds according to its `ProbeSpec`;
2. the listener owner maps to the tracked process identity or process group.

Runtime algorithm:

1. Select a candidate port from the model window.
2. Record reservation intent under registry transaction.
3. Start the owned foreground service.
4. Record process identity before readiness.
5. Probe endpoint.
6. Resolve platform listener ownership.
7. Match ownership to tracked pid, pgid, and start identity.
8. Mark ready only if ownership is verified.

Linux backend:

- use `/proc` socket inode and file descriptor ownership to map listener to pid;
- verify pid plus process start identity;
- verify pgid is the runtime-owned group.

macOS backend:

- use a platform socket-owner backend;
- containment is process-group based and weaker;
- fail closed with `PORT_UNVERIFIABLE` if ownership cannot be proven.

A successful TCP connect never proves readiness by itself.

## Typed Error Policy

Use the RFC error categories with typed payloads.

| Condition | Error code |
| --- | --- |
| Non-store model in normal mode | `MODEL_NOT_STORE_OUTPUT` |
| Malformed JSON, unknown fields, missing required fields | `MODEL_INVALID` |
| Admission contract failure | `MODEL_ADMISSION` |
| Runtime ABI or toolchain mismatch | `RUNTIME_ABI_MISMATCH` |
| Source dirty/fingerprint policy failure | `SOURCE_MISMATCH` |
| Target/capability mismatch | `PLATFORM_UNSUPPORTED` |
| Missing, undeclared, incompatible, or non-executable closure | `CLOSURE_MISSING` |
| Bind conflict with M0 fail policy | `PORT_CONFLICT` |
| Listener ownership cannot be matched | `PORT_UNVERIFIABLE` |
| State root cannot be created/written | `STATE_UNWRITABLE` |
| Cleanup target lacks matching marker or escapes state base | `STATE_UNOWNED` |
| Lease stale/conflict evidence | `LEASE_STALE` / `LEASE_CONFLICT` |
| Daemonization or process escape | `PROC_ESCAPE` |
| Readiness timeout | `READINESS_TIMEOUT` |
| Required secret behavior attempted | `SECRET_UNAVAILABLE` |
| Secret leak persistence blocked, if descriptor-only validation exists | `SECRET_LEAK_BLOCKED` |
| Run canceled | `CANCELED` |
| Cleanup refused by safety gates | `CLEANUP_REFUSED` |
| SQLite schema/integrity failure | `REGISTRY_CORRUPT` |

Unsupported M0 feature mapping:

| Unsupported feature | M0 behavior |
| --- | --- |
| Non-empty `secrets` | Reject as `MODEL_ADMISSION` with `unsupportedFeature = "secrets"`. |
| Secret injection requested | Reject as `MODEL_ADMISSION` or `SECRET_UNAVAILABLE` if resolution is attempted. |
| Workflows present or executed | Reject as `MODEL_ADMISSION` with `unsupportedFeature = "workflows"`. |
| Adapters/runtime adapter protocol | Reject as `MODEL_ADMISSION` with `unsupportedFeature = "adapters"`. |
| Slot other than `0` | Reject as `MODEL_ADMISSION`. |
| Service reuse / non-run-scoped lifetime | Reject as `MODEL_ADMISSION`. |
| Socket activation required | Reject as `PLATFORM_UNSUPPORTED`. |
| SQLite migration requested | Reject as `REGISTRY_CORRUPT` or `MODEL_ADMISSION`, depending on entry point. |

Every error payload should include `modelPath` and `computedModelHash` once raw model bytes were readable.

## Generated Views

M0 implements minimal generated views:

- `model`: prints or queries `model.json`;
- `schema`: emits runtime input/output schema derived from the model;
- `docs`: emits generated Markdown from model metadata;
- `capabilities`: emits `model.capabilities`.

Rules:

- views are projections only;
- views are not semantic authorities;
- runtime admits only `model.json`;
- deleting or mutating views must not change runtime admission.

## Acceptance Tests

M0 is accepted when these tests pass:

1. `examples/m0-minimal` compiles to `/nix/store/.../model.json`.
2. No required manifest is emitted or consumed.
3. Generated views derive from `model.json`.
4. Runtime ignores generated views during admission.
5. Runtime computes and records `computedModelHash`.
6. Exact `runtimeAbi` and `toolchainId` admits.
7. Mismatched `runtimeAbi` or `toolchainId` refuses.
8. Non-store model refuses in normal mode.
9. Non-store model admits only through unstable test/dev escape hatch.
10. Runtime works with `nix` unavailable once closures are realised.
11. Missing closure refuses.
12. Undeclared closure reference refuses.
13. Non-executable closure refuses.
14. Target-incompatible closure refuses.
15. One synthetic foreground service starts under owned process group.
16. Process record exists before readiness.
17. Readiness verifies endpoint ownership.
18. Dependent task runs only after readiness.
19. Registry records ordered events, run, service, process, port, and cleanup data.
20. Summary and logs are written under run root.
21. `ps` reconciles against OS process state before reporting.
22. `down` stops the owned process group.
23. `clean` deletes only marker-owned inactive state.
24. Non-empty secrets reject.
25. Workflows reject.
26. Adapters reject.
27. Service reuse rejects.
28. Slot `1` rejects.

## Negative Tests

Admission and runtime negative coverage:

- malformed model returns `MODEL_INVALID`;
- unknown fields return `MODEL_INVALID`;
- model path outside Nix store returns `MODEL_NOT_STORE_OUTPUT`;
- ABI mismatch returns `RUNTIME_ABI_MISMATCH`;
- toolchain mismatch returns `RUNTIME_ABI_MISMATCH`;
- target mismatch returns `PLATFORM_UNSUPPORTED`;
- source reject policy returns `SOURCE_MISMATCH`;
- closure missing returns `CLOSURE_MISSING`;
- executable bit missing returns `CLOSURE_MISSING`;
- unrelated process owns an open port and readiness returns `PORT_UNVERIFIABLE`;
- bind conflict with fail policy records collision and returns `PORT_CONFLICT`;
- service daemonization/double-fork returns `PROC_ESCAPE`;
- incompatible registry version returns `REGISTRY_CORRUPT`;
- cleanup refuses unmarked roots;
- cleanup refuses symlink/path traversal;
- cleanup refuses marker mismatch;
- cleanup refuses active lease/process/port refs;
- cleanup refuses protected persistent state;
- non-empty secrets reject;
- workflows/adapters/reuse/multi-slot reject.

## Proof Scripts

### Downstream-shaped minimal proof

`tests/m0/prove-downstream-minimal.sh` should:

1. build `examples/m0-minimal` through its public flake import;
2. assert resulting model path is under `/nix/store`;
3. assert no manifest is required;
4. run model/view commands;
5. run the runtime end to end through public surfaces only.

### Runtime-without-Nix proof

`tests/m0/prove-runtime-without-nix.sh` should:

1. build and realise model plus closures with Nix;
2. create a fake `nix` executable earlier in `PATH`;
3. make the fake executable fail and record if invoked;
4. run `nixfied-runtime` against the already-realised store model;
5. assert the run succeeds;
6. assert the fake `nix` sentinel was never touched.

## Platform Assumptions

M0 supports local Linux and macOS only.

Assumptions:

- local filesystem supports SQLite WAL and locking;
- tests set `NIXFIED_STATE_DIR` to a temp local directory;
- services are foreground-only;
- containment is process-group based;
- Linux port ownership uses `/proc`;
- macOS containment is weaker and must fail closed when ownership is not provable;
- no network filesystem guarantees;
- no cgroup v2, pidfd-only, subreaper, socket activation, daemon supervision, remote host, or multi-host support.

## Risk Register

| Risk | Mitigation |
| --- | --- |
| Port openness mistaken for ownership | Readiness fails unless OS owner matches tracked process identity. |
| PID reuse causes false liveness | Record pid, pgid, and platform start identity. |
| Runtime accidentally invokes Nix | Keep no Nix code paths in runtime and add fake `nix` sentinel test. |
| Closure not realised before runtime | Make closures derivation inputs in compile/prepare and verify at admission. |
| Model/view drift | Runtime reads only `model.json`; tests mutate/delete views. |
| Cleanup deletes user files | Canonical path checks plus marker, lease, process, and port gates. |
| ABI/toolchain constants drift | Define constants in Nix and Rust and test equality via generated model. |
| Source dirty policy becomes too broad | Default M0 to `warn`; use explicit `reject` only for negative proof. |
| Platform port owner backend is weak on macOS | Fail closed with `PORT_UNVERIFIABLE` unless ownership is proven. |
| Scope creep into workflows/adapters/secrets/reuse/multi-slot | Add explicit Nix assertions and runtime unsupported errors. |

## Unresolved Decisions Blocking Coding

These M0 decisions must be accepted before implementation starts:

1. Identity strings are `nixfied-toolchain:m0:1` and `nixfied-runtime-abi:m0:1`.
2. The single live workspace source policy defaults to `dirtyPolicy = "warn"`; explicit `reject` is allowed for negative tests.
3. The literal M0 "one ExecSpec" requirement is satisfied by one synthetic helper exec with typed operation bindings for service and task behavior.
4. Normal store root admission recognizes `/nix/store`; non-store admission is hidden unstable test/dev mode only.
5. Linux port ownership uses `/proc`; macOS fails closed unless the backend proves listener ownership.
6. Registry schema version is exactly `1`; incompatible versions fail and no migrations exist.
7. M0 service lifetime is run-scoped only; all reuse/borrower/persistent behavior is rejected.

## Commit And Review Workflow

Each implementation commit must have one architectural purpose and leave the repository coherent.

For every commit:

1. The implementing engineer prepares the patch.
2. A separate review engineer inspects the diff before commit.
3. Review checks correctness against `RFC_v2.md`, M0 scope discipline, tests, and regressions.
4. Blocking findings are fixed before commit.
5. Any accepted risk is explicitly recorded in the commit notes or follow-up issue.
6. Only then is the commit created.

## Proposed Commit Sequence

| Commit | Purpose | Expected review focus |
| --- | --- | --- |
| `scaffold: add greenfield Nix and Rust workspace` | Add flake, Cargo workspace, crate skeletons, and base directories. | No legacy compatibility, correct RFC layout, no semantic sidecars. |
| `model: add M0 constants and serde contract` | Add `nixfied-model` types and exact ABI/toolchain constants. | Strict schema, no self-hash, host paths absent from model types. |
| `model: add typed Nix module and compiler emission` | Add Nix module options, compiler passes, closure metadata, and store `model.json` emission. | Typed validation, `PREPARE-1`, no manifest, no host-absolute placement. |
| `model: generate minimal views` | Add `schema`, `docs`, and `capabilities` views. | Views are disposable projections and not runtime authorities. |
| `runtime: admit store model and compute hash` | Add loader, raw hash, origin/ABI/toolchain/target/source/closure/secrets checks. | No Nix invocation, fail-before-process behavior, typed errors. |
| `registry: add sqlite schema v1` | Add WAL DB, ordered events, run/service/process/port/cleanup records. | Total ordering, schema version `1`, no migrations, local FS assumptions. |
| `state: derive roots and marker-gated cleanup` | Add host placement, marker file, logs, summary, and cleanup refusal rules. | Path confinement, marker ownership, no unsafe deletion. |
| `service: run synthetic foreground service` | Add process-group spawn and typed start/ready/stop lifecycle plumbing. | Foreground-only, process record before readiness, daemon escape refusal. |
| `service: verify endpoint ownership and run task` | Add port owner backend, readiness gate, and dependent task execution. | Ownership proof, not just TCP probe; task waits for ready service. |
| `runtime: add ps down clean reconciliation` | Add OS reconciliation, owned process-group shutdown, and cleanup command flow. | OS is liveness authority; `down` stops owned process group only. |
| `tests: add M0 spine acceptance proof` | Add downstream example, negative tests, no-Nix proof, and full end-to-end proof. | RFC M0 coverage, deferred features remain rejected. |

