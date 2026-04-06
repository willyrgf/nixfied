{
  pkgs,
  system,
  projectRoot,
  projectModules,
  extraModules ? [ ],
  localOverrides ? [ ],
  selectedServices ? null,
  frameworkSourceRevision ? import ./framework-revision.nix {
    sourcePath = ../../.;
    metadataPath = ../../VENDORED.txt;
  },
}:
let
  canonical = import ./canonical.nix { inherit (pkgs) lib; };
  compiledCore = import ./mkCompiledCore.nix {
    inherit
      pkgs
      system
      projectRoot
      projectModules
      extraModules
      localOverrides
      frameworkSourceRevision
      selectedServices
      ;
  };

  runtimeArtifacts = import ./materializeExecution.nix {
    inherit
      pkgs
      projectRoot
      ;
    compiledCore = compiledCore;
  };

  coreSurfaces = import ./mkCoreSurfaces.nix {
    inherit
      pkgs
      canonical
      ;
    compiledCore = compiledCore;
  };

  runtimeApps = import ./mkRuntimeAppSet.nix {
    inherit pkgs;
    runtimeProgram = "${runtimeArtifacts.runtimeEngine}/bin/nixfied-runtime";
    model = compiledCore.model;
    contractBundle = compiledCore.contractBundle;
  };

  apps =
    runtimeApps
    // coreSurfaces.apps
    // {
      default = if runtimeApps ? help then runtimeApps.help else coreSurfaces.apps.help;
    };

  packages = coreSurfaces.packages // {
    default = pkgs.runCommand "nixfied-default" { } ''
      mkdir -p "$out/bin"
      ln -s ${apps.default.program} "$out/bin/default"
    '';
  };
in
{
  model = compiledCore.model;
  statePolicy = compiledCore.model.state.policy;
  stateHash = compiledCore.stateHash;
  runtimeHash = runtimeArtifacts.runtimeHash or compiledCore.model.identity.evalHash;
  tasks = compiledCore.model.tasks;
  services = runtimeArtifacts.services;
  serviceDefinitions = compiledCore.resolved.services or { };
  serviceCatalog = compiledCore.model.serviceCatalog;
  workflows = compiledCore.model.workflows;
  features = compiledCore.model.features;
  introspectionGraph = compiledCore.introspectionGraph;
  introspectionBundle = compiledCore.introspectionBundle;
  serviceSurfaceCatalog = (compiledCore.model.compiled or { }).serviceSurfaceCatalog or { };
  serviceApis =
    ((compiledCore.model.compiled or { }).serviceSurfaceCatalog or { }).serviceApis or { };

  inherit
    apps
    packages
    ;
  checks = coreSurfaces.checks;
  devShells = coreSurfaces.devShells;
  schema = coreSurfaces.schema;
}
