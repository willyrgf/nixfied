# Stage 4 Typed Service Contracts

Stage 4 moves public service contract authority into typed service module data under `nixfied/modules/services/*`.

The typed schema is a hidden read-only `contract` option per built-in service. Each contract carries `version`, `service`, `summary`, `details`, `ownerFile`, `adapter`, `artifacts`, `runtimePrimitives`, and typed `operations`. Each operation declares only public contract metadata plus the runtime adapter op it projects to through `runtimeOp`, with `preOps` and `postOps` used for composed public operations.

The runtime adapter ABI is intentionally small: runtime service modules now export `{ version = 1; operations = { ... }; }`. They are private implementation only. Each adapter operation is a script/path/derivation keyed by the runtime op names referenced from the typed contract, and observability scripts are injected by runtime materialization instead of stored as public contract data in runtime modules.

Deleted authority in this wave:

- `nixfied/compiler/compile-service-surface-catalog.nix` no longer imports runtime service modules through fake project or slot context.
- `nixfied/framework/core/serviceModulePath.nix` is removed.
- `nixfied/framework/runtime/helpers/service-module.nix` is removed as a public contract seam.
- `nixfied/framework/runtime/services/*/default.nix` no longer export `publicApi` or `serviceModule`.

No dual authority remains because the compiler reads public service contracts only from resolved typed module data, and runtime hooks/apps/service-set wrappers project only from the compiled contract catalog plus the private adapter operation map referenced by `contract.adapter.module`.
