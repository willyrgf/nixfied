# Actual adapter publications. Native module functions retain all behavior.
[
  {
    kind = "module";
    scope = "adapter";
    name = "synthetic";
    description = "Synthetic TCP service and smoke task for a minimal project.";
    usage = "imports = [ adapters.synthetic ];";
    contributes = "The synthetic service, helper closure, and smoke task; override through native module merging.";
    binding = import ./synthetic.nix;
  }
  {
    kind = "module";
    scope = "adapter";
    name = "postgres";
    description = "PostgreSQL service with prepare task and protocol readiness probes.";
    usage = "imports = [ adapters.postgres ];";
    contributes = "The postgres service, packaged tools, preparation task and smoke-query task. Native overrides configure their generic declarations.";
    binding = import ./postgres.nix;
  }
  {
    kind = "module";
    scope = "adapter";
    name = "reth";
    description = "Reth service with explicitly owned HTTP, WebSocket and authenticated RPC endpoints.";
    usage = "imports = [ adapters.reth ];";
    contributes = "The reth service, packaged tools and reth-smoke task. Listener configuration stays in the native adapter.";
    binding = import ./reth.nix;
  }
]
