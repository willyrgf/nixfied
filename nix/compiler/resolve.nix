{
  lib,
  pkgs,
  system,
  module,
}:

lib.evalModules {
  specialArgs = {
    inherit pkgs system;
    adapters = import ../adapters/default.nix;
    nixfiedLib = import ../lib/compose.nix { inherit lib; };
  };
  modules = [
    ../modules/default.nix
    module
  ];
}
