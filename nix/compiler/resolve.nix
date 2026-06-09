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
  };
  modules = [
    ../modules/default.nix
    module
  ];
}
