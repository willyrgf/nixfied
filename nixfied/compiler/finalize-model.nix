{ lib, canonical }:
{
  system,
  projectRoot,
  resolved,
  statePolicy,
  runtime,
  apiCatalog ? null,
  execution ? null,
  runtimeMetadata ? null,
  serviceSurfaceCatalog ? null,
  services ? null,
  serviceCatalog,
  serviceSets,
  apps,
  tasks,
  workflows,
  features,
  views,
}:
let
  identityBase = {
    projectId = resolved.identity.projectId;
    projectName = resolved.identity.projectName;
    description = resolved.identity.description;
    system = system;
  };

  modelPayload = {
    schema = {
      kind = "nixfied-model";
      version = 7;
    };

    identity = identityBase;

    runtime = runtime;

    serviceCatalog = serviceCatalog;
    serviceSets = serviceSets;
    apps = apps;
    tasks = tasks;
    workflows = workflows;
    features = features;
    views = {
      apps = views.apps;
      help = views.help;
      docs = views.docs;
      features = views.features;
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
        services = services;
      };

  evalHash = canonical.hashCanonical {
    schema = {
      kind = "nixfied-model-eval";
      version = 1;
    };
    identity = resolved.identity;
    runtime = runtime;
    services = serviceCatalog;
    serviceSets = serviceSets;
    apps = apps;
    tasks = tasks;
    workflows = workflows;
    features = features;
  };

  modelPayloadWithEvalHash = modelPayload // {
    identity = modelPayload.identity // {
      evalHash = evalHash;
    };
  };

  model = canonical.canonicalize (
    modelPayloadWithEvalHash
    // {
      compiled = {
        apiCatalog = apiCatalog;
        execution = execution;
        runtimeMetadata = runtimeMetadata;
        serviceSurfaceCatalog = serviceSurfaceCatalog;
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
