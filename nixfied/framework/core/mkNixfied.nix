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
    inherit compiledCore;
  };

  coreSurfaces = import ./mkCoreSurfaces.nix {
    inherit
      pkgs
      canonical
      ;
    inherit compiledCore;
  };

  runtimeApps = import ./mkRuntimeAppSet.nix {
    inherit pkgs;
    runtimeProgram = "${runtimeArtifacts.runtimeEngine}/bin/nixfied-runtime";
    inherit (compiledCore) model;
    inherit (compiledCore) contractBundle;
  };

  apps =
    runtimeApps
    // coreSurfaces.apps
    // {
      default = runtimeApps.help or coreSurfaces.apps.help;
    };

  packages = coreSurfaces.packages // {
    default = pkgs.runCommand "nixfied-default" { } ''
      mkdir -p "$out/bin"
      ln -s ${apps.default.program} "$out/bin/default"
    '';
  };
in
{
  inherit (compiledCore) model;
  statePolicy = compiledCore.model.state.policy;
  inherit (compiledCore) stateHash;
  runtimeHash = runtimeArtifacts.runtimeHash or compiledCore.model.identity.evalHash;
  inherit (compiledCore.model) tasks;
  inherit (runtimeArtifacts) services;
  serviceDefinitions = compiledCore.resolved.services or { };
  inherit (compiledCore.model) serviceCatalog;
  inherit (compiledCore.model) workflows;
  inherit (compiledCore.model) features;
  inherit (compiledCore) introspectionGraph;
  inherit (compiledCore) introspectionBundle;
  serviceSurfaceCatalog = (compiledCore.model.compiled or { }).serviceSurfaceCatalog or { };
  serviceApis =
    ((compiledCore.model.compiled or { }).serviceSurfaceCatalog or { }).serviceApis or { };

  inherit
    apps
    packages
    ;
  inherit (coreSurfaces) checks;
  inherit (coreSurfaces) devShells;
  inherit (coreSurfaces) schema;
}
