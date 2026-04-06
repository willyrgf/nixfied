{
  pkgs,
  canonical,
  modules,
  system,
  projectRoot,
  frameworkSourceRevision,
}:
let
  inherit (pkgs) lib;
  contracts = import ../contracts {
    inherit
      lib
      canonical
      ;
  };

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
  compileServiceSets = import ./compile-service-sets.nix {
    inherit
      lib
      canonical
      idLib
      ;
  };
  compileServiceSurfaceCatalog = import ./compile-service-surface-catalog.nix {
    inherit
      lib
      pkgs
      ;
  };
  compileServices = import ./compile-services.nix { inherit lib; };

  compileTasks = import ./compile-tasks.nix {
    inherit
      lib
      canonical
      idLib
      ;
  };

  compileValidationIr = import ./compile-validation-ir.nix {
    inherit
      lib
      canonical
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
  compileApiCatalog = import ./compile-api-catalog.nix {
    inherit
      lib
      canonical
      ;
  };
  compileExecution = import ./compile-execution.nix {
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
  compileIntrospectionBundle = import ./compile-introspection-bundle.nix {
    inherit
      lib
      canonical
      ;
  };

  compileViews = import ./compile-views.nix { inherit lib; };
  compileContractBundle = import ./compile-contract-bundle.nix {
    inherit
      lib
      canonical
      contracts
      ;
  };
  frameworkContractDefinitions = import ../framework/contracts/default-definitions.nix {
    contracts = contracts.types;
  };

  finalizeModel = import ./finalize-model.nix {
    inherit canonical;
    _lib = lib;
  };
in
rec {
  compileCore =
    {
      projectModules,
      extraModules ? [ ],
      localOverrides ? [ ],
      selectedServices ? null,
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

      legacyLocalDefault =
        let
          relativePath = "nixfied/local/default.nix";
          projectPath = builtins.unsafeDiscardStringContext "${builtins.toString projectRoot}/${relativePath}";
          templateContents = builtins.readFile ../local/default.nix;
          present = builtins.pathExists projectPath;
          contents = if present then builtins.readFile projectPath else "";
          customized = present && contents != templateContents;
          status =
            if !present then
              "missing"
            else if customized then
              "customized-inactive"
            else
              "template-inactive";
          message =
            if !present then
              "legacy local/default.nix is absent"
            else if customized then
              "legacy local/default.nix differs from the framework template and is not loaded by nixfied"
            else
              "legacy local/default.nix matches the framework template and is not loaded by nixfied";
        in
        {
          path = relativePath;
          inherit
            present
            customized
            status
            message
            ;
          active = false;
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

      resolvedServices = compileServices {
        inherit pkgs;
        inherit selectedServices;
        resolved = resolvedModuleGraph.config;
      };

      serviceSets = compileServiceSets {
        inherit
          projectRoot
          serviceCatalog
          ;
        resolvedIdentity = resolvedModuleGraph.config.identity;
        baseStatePolicy = statePolicy;
        resolved = resolvedModuleGraph.config;
      };

      serviceSurfaceCatalog = compileServiceSurfaceCatalog {
        services = resolvedServices;
        serviceDefinitions = resolvedModuleGraph.config.services or { };
      };

      taskCompilation = compileTasks {
        resolved = resolvedModuleGraph.config;
        inherit runtime;
      };

      inherit (taskCompilation) tasks;

      workflows = compileWorkflows {
        resolved = resolvedModuleGraph.config;
        inherit tasks;
        inherit serviceSets;
        inherit (taskCompilation) allTasks;
        inherit (taskCompilation) declaredTaskIds;
        inherit (taskCompilation) prunedTaskIds;
        inherit (taskCompilation) pruneReasonsByTaskId;
      };

      contractBundle = compileContractBundle {
        resolved = resolvedModuleGraph.config;
        frameworkDefinitions = frameworkContractDefinitions;
      };

      validationIr = compileValidationIr {
        inherit contractBundle;
      };

      apps = compileApps {
        resolved = resolvedModuleGraph.config;
        _serviceSets = serviceSets;
        inherit
          serviceSurfaceCatalog
          tasks
          workflows
          contractBundle
          ;
      };

      apiCatalog = compileApiCatalog {
        resolved = resolvedModuleGraph.config;
        inherit
          tasks
          apps
          workflows
          serviceSets
          ;
      };

      execution = compileExecution {
        resolvedIdentity = resolvedModuleGraph.config.identity;
        inherit runtime;
        state = {
          policy = statePolicy;
          registry = {
            schemaVersion = 1;
          };
        };
        inherit
          serviceCatalog
          serviceSets
          apps
          tasks
          workflows
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
          execution
          serviceSets
          tasks
          workflows
          serviceCatalog
          serviceSurfaceCatalog
          features
          ;
        resolved = resolvedModuleGraph.config;
        localOverridesActive = localOverrides != [ ];
        localOverrideCount = builtins.length localOverrides;
        inherit legacyLocalDefault;
      };

      introspectionBundle = compileIntrospectionBundle {
        inherit introspectionGraph;
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
          statePolicy
          runtime
          serviceCatalog
          serviceSets
          apps
          tasks
          workflows
          features
          views
          apiCatalog
          execution
          serviceSurfaceCatalog
          ;
        _projectRoot = projectRoot;
        resolved = resolvedModuleGraph.config;
      };
    in
    finalized
    // {
      resolved = resolvedModuleGraph.config;
      inherit tasks;
      inherit workflows;
      inherit apps;
      inherit introspectionGraph;
      inherit introspectionBundle;
      inherit views;
      inherit runtime;
      inherit statePolicy;
      inherit serviceCatalog;
      inherit resolvedServices;
      inherit serviceSets;
      inherit (finalized.model.compiled) serviceSurfaceCatalog;
      inherit features;
      inherit contractBundle;
      inherit validationIr;
      inherit legacyLocalDefault;
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
        inherit services;
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
