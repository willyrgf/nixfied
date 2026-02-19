# Detailed Architecture

## Model

`nixfiedModel` is compiled from typed modules and is the only source for generated views.

Top-level shape:

- `schema`
- `identity`
- `runtime`
- `services`
- `tasks`
- `workflows`
- `views`
- `state`

## Compiler Passes

Pass order:

1. `resolve-modules`
2. `normalize-runtime`
3. `compile-services`
4. `compile-tasks`
5. `compile-workflows`
6. `compile-views`
7. `finalize-model`

Each pass is pure and deterministic.

## Determinism

- Canonical rendering is Nix-native (`toCanonicalNix`), not JSON.
- Attrset keys are recursively sorted.
- List ordering is preserved.
- Function values are rejected in canonicalized subtrees.
- `stateHash` is the SHA-256 digest of canonical model rendering.

## Runtime Boundary

Executor guarantees:

- deterministic env initialization
- hermetic `PATH` from declared `runtimeInputs`
- deterministic defaults (`LANG`, `LC_ALL`, `TZ`, `umask`)
- model-backed task and workflow dispatch only

## Registry

Event log location:

- `$REGISTRY_ROOT/events.ndjson`

Rules:

- one compact JSON object per line
- newline-delimited and newline-terminated
- append-only writes
- stable key order
- monotonic contiguous `seq`
- RFC3339 UTC `ts`

Run IDs are deterministic and include collision suffixing policy (`-cNNN`) for active-run conflicts.

## Flake Surfaces

Introspection:

- `.#model`
- `.#stateHash`
- `.#tasks`
- `.#task::<id>`
- `.#schema`

Dispatcher:

- `.#run-task`
- `.#run-workflow`

## Testing

Determinism gates live in `tests/framework/` and are published via `flake checks`.
