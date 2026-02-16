{
  pkgs ? null,
}:

let
  conf = import ./conf.nix { inherit pkgs; };
  project = conf.project or { };
  frameworkLib = import ../.framework/lib {
    inherit pkgs;
    project = conf;
  };
  commandLib = import ./lib/command.nix {
    inherit project;
    appApi = frameworkLib.appApi;
  };
  catalog = import ./catalog.nix { inherit project commandLib; };
  mkPart = path: import path { inherit pkgs project commandLib catalog; };
  parts = [
    conf
    (mkPart ./dev.nix)
    (mkPart ./test.nix)
    (mkPart ./prod.nix)
    (mkPart ./quality.nix)
    (mkPart ./format.nix)
    (mkPart ./ci.nix)
  ];
in
pkgs.lib.foldl' pkgs.lib.recursiveUpdate { } parts
