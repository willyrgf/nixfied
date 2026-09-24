{
  lib,
  pkgs,
  system,
  module,
}:
let
  providers = import ../modules/providers.nix { inherit lib pkgs system; };
in
lib.evalModules {
  specialArgs = providers.bindings;
  modules = [
    ../modules/default.nix
    module
  ];
}
