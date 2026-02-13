{
  pkgs ? null,
}:

let
  conf = import ./conf.nix { inherit pkgs; };
  project = conf.project or { };
  commandLib = import ./lib/command.nix { inherit project; };
  mkPart = path: import path { inherit pkgs project commandLib; };
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
