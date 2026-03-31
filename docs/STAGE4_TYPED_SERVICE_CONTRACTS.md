# Stage 4 Typed Service Contracts

Status: implemented

Stage 4 moves public service contract authority into typed service module data under `nixfied/modules/services/*`.

The typed schema is a hidden read-only `contract` option plus a hidden read-only `implementation` option per built-in service. Public contracts carry `version`, `service`, `summary`, `details`, `ownerFile`, `artifacts`, `runtimePrimitives`, and typed `operations`. Each operation declares only public contract metadata plus the runtime op it projects to through `runtimeOp`, with `preOps` and `postOps` used for composed public operations.

The private runtime implementation ABI is intentionally small: service runtime modules now export `{ version = 1; operations = { ... }; }`. They are private implementation only and now live under `nixfied/modules/services/runtime/*`. Each runtime operation is a script/path/derivation keyed by the runtime op names referenced from the typed contract, and observability scripts are injected by runtime materialization instead of stored as public contract data in runtime modules.

Deleted authority in this wave:

- `nixfied/compiler/compile-service-surface-catalog.nix` no longer imports runtime service modules through fake project or slot context.
- `nixfied/framework/core/serviceModulePath.nix` is removed.
- `nixfied/framework/runtime/helpers/service-module.nix` is removed as a public contract seam.
- `nixfied/framework/runtime/services/*/default.nix` no longer act as the live authority for built-in service implementations.

No dual authority remains because the compiler reads public service contracts only from resolved typed module data, and runtime hooks/apps/service-set wrappers project only from the compiled contract catalog plus private implementation data from `serviceDefinitions.<name>.implementation.module`.
