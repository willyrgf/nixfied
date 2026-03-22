{
  pkgs,
  canonical,
  modules,
  system,
  projectRoot,
  frameworkSourceRevision,
}:
let
  lib = pkgs.lib;

  idLib = import ./id.nix {
    inherit
      lib
      canonical
      ;
  };

  resolveModules = import ./resolve-modules.nix {
    inherit
      lib
      modules
      ;
  };

  compileStatePolicy = import ./compile-state-policy.nix { inherit lib; };
  normalizeRuntime = import ./normalize-runtime.nix { inherit lib; };
  compileServiceCatalog = import ./compile-service-catalog.nix { inherit lib; };
  compileServiceSurfaceCatalog = import ./compile-service-surface-catalog.nix { inherit lib; };
  compileServices = import ./compile-services.nix { inherit lib; };

  compileTasks = import ./compile-tasks.nix {
    inherit
      lib
      canonical
      idLib
      ;
  };

  compileWorkflows = import ./compile-workflows.nix {
    inherit
      lib
      canonical
      idLib
      ;
  };

  compileFeatures = import ./compile-features.nix {
    inherit
      lib
      canonical
      ;
  };

  compileApps = import ./compile-apps.nix {
    inherit
      lib
      canonical
      ;
  };

  compileAppExecutionManifests = import ./compile-app-execution-manifests.nix {
    inherit
      lib
      canonical
      ;
  };

  compileIntrospectionGraph = import ./compile-introspection-graph.nix {
    inherit
      lib
      canonical
      ;
  };

  compileViews = import ./compile-views.nix { inherit lib; };
  compileSelectionIndex = import ./compile-selection-index.nix { inherit lib; };

  finalizeModel = import ./finalize-model.nix {
    inherit
      lib
      canonical
      ;
  };
in
rec {
  compileCore =
    {
      projectModules,
      extraModules ? [ ],
      localOverrides ? [ ],
    }:
    let
      resolvedModuleGraph = resolveModules {
        inherit
          pkgs
          system
          projectRoot
          frameworkSourceRevision
          projectModules
          extraModules
          localOverrides
          ;
      };

      statePolicy = compileStatePolicy {
        inherit
          projectRoot
          ;
        resolved = resolvedModuleGraph.config;
      };

      runtime = normalizeRuntime {
        resolved = resolvedModuleGraph.config;
        inherit statePolicy;
      };

      serviceCatalog = compileServiceCatalog {
        resolved = resolvedModuleGraph.config;
      };

      serviceSurfaceCatalog = compileServiceSurfaceCatalog {
        inherit serviceCatalog;
      };

      taskCompilation = compileTasks {
        resolved = resolvedModuleGraph.config;
        inherit runtime;
      };

      tasks = taskCompilation.tasks;

      workflows = compileWorkflows {
        resolved = resolvedModuleGraph.config;
        inherit tasks;
        allTasks = taskCompilation.allTasks;
        declaredTaskIds = taskCompilation.declaredTaskIds;
        prunedTaskIds = taskCompilation.prunedTaskIds;
        pruneReasonsByTaskId = taskCompilation.pruneReasonsByTaskId;
      };

      apps = compileApps {
        resolved = resolvedModuleGraph.config;
        inherit tasks;
      };

      selectionIndex = compileSelectionIndex {
        inherit
          tasks
          workflows
          serviceCatalog
          ;
      };

      appExecutionManifests = compileAppExecutionManifests {
        resolvedIdentity = resolvedModuleGraph.config.identity;
        runtime = runtime;
        state = {
          policy = statePolicy;
          registry = {
            schemaVersion = 1;
          };
        };
        inherit
          serviceCatalog
          apps
          tasks
          workflows
          selectionIndex
          ;
      };

      features = compileFeatures {
        inherit
          projectRoot
          runtime
          apps
          tasks
          workflows
          ;
        services = serviceCatalog;
      };

      introspectionGraph = compileIntrospectionGraph {
        inherit
          projectRoot
          statePolicy
          runtime
          apps
          appExecutionManifests
          tasks
          workflows
          serviceCatalog
          serviceSurfaceCatalog
          features
          selectionIndex
          ;
        resolved = resolvedModuleGraph.config;
        localOverridesActive = localOverrides != [ ];
        localOverrideCount = builtins.length localOverrides;
      };

      views = compileViews {
        inherit projectRoot;
        resolved = resolvedModuleGraph.config;
        inherit
          statePolicy
          features
          runtime
          apps
          tasks
          workflows
          ;
        services = serviceCatalog;
      };

      finalized = finalizeModel {
        inherit
          system
          projectRoot
          statePolicy
          runtime
          serviceCatalog
          apps
          tasks
          workflows
          features
          views
          ;
        resolved = resolvedModuleGraph.config;
      };
    in
    finalized
    // {
      resolved = resolvedModuleGraph.config;
      tasks = tasks;
      workflows = workflows;
      apps = apps;
      appExecutionManifests = appExecutionManifests;
      introspectionGraph = introspectionGraph;
      views = views;
      runtime = runtime;
      statePolicy = statePolicy;
      serviceCatalog = serviceCatalog;
      serviceSurfaceCatalog = serviceSurfaceCatalog;
      features = features;
      selectionIndex = selectionIndex;
    };

  compileServicesResolved =
    resolved:
    compileServices {
      inherit pkgs resolved;
    };

  compile =
    args:
    let
      core = compileCore args;
      services = compileServicesResolved core.resolved;
      runtimeHash = canonical.hashCanonical {
        schema = {
          kind = "nixfied-runtime";
          version = 1;
        };
        services = services;
      };
    in
    core
    // {
      inherit
        services
        runtimeHash
        ;
    };
}
