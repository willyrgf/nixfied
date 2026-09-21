{
  lib,
  pkgs,
  system,
  module,
}:
let
  authoring = import ../meta/authoring.nix { inherit lib pkgs system; };
in
lib.evalModules {
  specialArgs = authoring.specialArgs;
  modules = [
    ../modules/default.nix
    module
  ];
}
