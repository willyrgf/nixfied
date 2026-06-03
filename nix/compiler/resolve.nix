{
  lib,
  pkgs,
  system,
  module,
}:

lib.evalModules {
  specialArgs = {
    inherit pkgs system;
  };
  modules = [
    ../modules/default.nix
    module
  ];
}
