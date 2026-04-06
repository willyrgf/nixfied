{ canonical, ... }:
{
  system,
  resolved,
  statePolicy,
  runtime,
  apiCatalog ? null,
  execution ? null,
  serviceSurfaceCatalog ? null,
  services ? null,
  serviceCatalog,
  serviceSets,
  apps,
  tasks,
  workflows,
  features,
  views,
  ...
}:
let
  identityBase = {
    inherit (resolved.identity) projectId;
    inherit (resolved.identity) projectName;
    inherit (resolved.identity) description;
    inherit system;
  };

  modelPayload = {
    schema = {
      kind = "nixfied-model";
      version = 7;
    };

    identity = identityBase;

    inherit runtime;

    inherit serviceCatalog;
    inherit serviceSets;
    inherit apps;
    inherit tasks;
    inherit workflows;
    inherit features;
    views = {
      inherit (views) apps;
      inherit (views) help;
      inherit (views) docs;
      inherit (views) features;
    };

    state = {
      policy = statePolicy;
      registry = {
        schemaVersion = 1;
      };
    };
  };

  runtimeHash =
    if services == null then
      null
    else
      canonical.hashCanonical {
        schema = {
          kind = "nixfied-runtime";
          version = 1;
        };
        inherit services;
      };

  evalHash = canonical.hashCanonical {
    schema = {
      kind = "nixfied-model-eval";
      version = 1;
    };
    inherit (resolved) identity;
    inherit runtime;
    services = serviceCatalog;
    inherit serviceSets;
    inherit apps;
    inherit tasks;
    inherit workflows;
    inherit features;
  };

  modelPayloadWithEvalHash = modelPayload // {
    identity = modelPayload.identity // {
      inherit evalHash;
    };
  };

  model = canonical.canonicalize (
    modelPayloadWithEvalHash
    // {
      compiled = {
        inherit apiCatalog;
        inherit execution;
        inherit serviceSurfaceCatalog;
      };
    }
  );

  stateHash = canonical.hashCanonical modelPayloadWithEvalHash;
in
{
  inherit
    model
    stateHash
    runtimeHash
    ;
}
