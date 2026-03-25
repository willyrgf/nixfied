{ lib, canonical }:
{
  system,
  projectRoot,
  resolved,
  statePolicy,
  runtime,
  apiCatalog ? null,
  runtimeManifests ? null,
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

  model = canonical.canonicalize {
    schema = {
      kind = "nixfied-model";
      version = 6;
    };

    identity = {
      projectId = resolved.identity.projectId;
      projectName = resolved.identity.projectName;
      description = resolved.identity.description;
      system = system;
      evalHash = evalHash;
    };

    runtime = runtime;

    serviceCatalog = serviceCatalog;
    serviceSets = serviceSets;
    apps = apps;
    tasks = tasks;
    workflows = workflows;
    features = features;
    compiled = {
      apiCatalog = apiCatalog;
      runtimeManifests = runtimeManifests;
      serviceSurfaceCatalog = serviceSurfaceCatalog;
    };

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

  stateHash = canonical.hashCanonical (builtins.removeAttrs model [ "compiled" ]);
in
{
  inherit
    model
    stateHash
    runtimeHash
    ;
}
