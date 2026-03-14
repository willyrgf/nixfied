# Workflow Reuse

This guide is the short, normative reference for reusing Nixfied workflow
orchestration in downstream projects.

## Current Semantics

- `run-workflow` parses one passthrough argv list and forwards the same argv to
  `preRun`, workflow units, and `postRun`.
- Workflow units do not support per-step args, env, or outputs today.
- Workflow units model ordering and scheduling, not step-to-step dataflow.
- Exported flake apps come from task apps. Service operation contracts exist in
  the framework, but they are not exported as flake apps in this repository.

## Current Limits

- Use a workflow when every phase can share the same invocation args.
- Do not use a workflow alone when one phase needs service startup metadata,
  another phase needs a different selector, and the public app must preserve a
  strict machine-output stdout contract.
- Port env names in generic task execution are derived from normalized model
  keys. For example, `heliosRpc` becomes `HELIOSRPC_PORT`.

## Recommended Pattern For Machine-Output Apps

Use a public wrapper task plus hidden setup and teardown tasks.

The public wrapper should:

1. Create temporary handoff state for non-secret runtime ownership metadata.
2. Invoke hidden service setup/start logic.
3. Invoke the real machine-output task and capture its stdout.
4. Validate the final payload shape if the app contract is stricter than "valid
   JSON".
5. Invoke hidden teardown logic in all exit paths.
6. Replay only the final validated payload to stdout.

The hidden setup/teardown tasks should:

- reuse framework helpers where possible
- keep secrets out of persisted handoff files
- accept the extra args that workflows cannot assign per step

## Ready And Health

- `task.ops.ready` and `task.ops.health` are reusable today.
- They consume the same canonical per-service probe plans as direct service
  `health` / `ready`.
- Probe overrides live under `nixfied.services.<name>.probes.{health,ready}`.
- They accept one `--service` selector per invocation.
- `--source` requires a concrete `--service`.
- `task.ops.ready` stays one-shot even when `nixfied.services.<name>.probes.ready.wait`
  is enabled for the direct service check.
- If a workflow needs polling readiness semantics, wrap the direct service
  `ready` operation in a task instead of relying on `task.ops.ready`.
- If a workflow needs different service selectors in different phases, wrap
  those invocations in separate tasks.
