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
  inherit (pkgs) lib;
  modules = import ../../modules;
  canonical = import ./canonical.nix { inherit lib; };

  compiler = import ../../compiler {
    inherit
      pkgs
      canonical
      modules
      system
      projectRoot
      frameworkSourceRevision
      ;
  };

  compiled = compiler.compileCore {
    inherit
      projectModules
      extraModules
      localOverrides
      selectedServices
      ;
  };
in
compiled
