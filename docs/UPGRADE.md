# Upgrade Notes

## Architectural Shift

This upgrade lands the stack rethink end state.

The framework now has:

- one compiler-owned execution representation
- one runtime-owned public execution entry
- thin flake apps that exec runtime directly
- compiler-owned semantic publication for execution, service ABI, validation IR, contract bundle/docs, and introspection assets

Compatibility shims were intentionally removed.

## Removed Public Surfaces

The following public contracts no longer exist:

- `SKIP_<SERVICE>`
- `SVC_<SERVICE>_<OP>`
- `svcset::*`
- `services-start`
- `services-stop`
- `services-status`
- `services-export`
- launcher recursion and selected-app manifest behavior as a public surface

## Surviving Replacements

Use these instead:

- Runtime exclusion: `--exclude-services <csv>`
- Static graph exclusion: `nixfied.graph.excludedServices`
- Public service ABI: `svc::<service>::<op>`
- In-task service invocation: `svc <service> <op>`

## Model and Artifact Changes

Main changes:

- `model.serviceCatalog` remains the cheap exported service view
- heavy service runtime remains internal as `compiled.services`
- canonical execution now lives in `compiled.execution`
- canonical service ABI catalog now lives in `compiled.serviceSurfaceCatalog`
- validation IR, contract bundle/docs, and introspection graph/bundle are published compiler-owned artifacts
- `stateHash` continues to represent the cheap canonical model
- `runtimeHash` fingerprints heavy runtime inputs used by runtime

## Runtime Changes

Public task, workflow, and service surfaces now exec one runtime engine directly.

Runtime still preserves:

- run inventory
- stop controls
- summary generation
- process setup and process-group lifecycle
- artifact placement
- env isolation
- ephemeral execution
- deterministic runtime assessment

## Downstream Impact

Adjust downstream code if it depended on removed public surfaces.

Common migrations:

- Replace `SKIP_<SERVICE>` with `--exclude-services <csv>`
- Replace `SVC_<SERVICE>_<OP>` with `svc::<service>::<op>` or `svc <service> <op>`
- Replace `svcset::*` and `services-*` invocations with explicit `svc::<service>::<op>` calls
- Remove dead service-operation `hook` / `exposeHook` config fields
- Stop depending on app-scoped execution manifests or launcher-specific selection behavior
- Stop assuming packaging owns semantic service selection

## Checklist

- Replace any `SKIP_<SERVICE>` usage.
- Replace any `SVC_<SERVICE>_<OP>` usage.
- Replace any `svcset::*` or `services-*` usage.
- Remove any service-operation `hook` or `exposeHook` configuration.
- Use `nixfied.graph.excludedServices` for repo-owned static graph exclusion.
- Use `--exclude-services <csv>` for invocation-time exclusion.
- Use `svc::<service>::<op>` or in-task `svc <service> <op>` for service operations.
- If you previously consumed deep runtime service details from public outputs, move that logic to compiler-owned artifacts instead of packaging-generated wrappers.

## Recommended Validation

- `nix run .#help`
- `nix run .#features`
- `nix run .#test -- --mode full --summary`
- Re-run one representative task/workflow with `--exclude-services`
- Re-run one representative service operation through `svc::<service>::<op>`
