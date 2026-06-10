{
  lib,
  pkgs,
  system,
  module,
}:

let
  constants = import ../spec/constants.nix;
  evaluated = import ./resolve.nix {
    inherit
      lib
      pkgs
      system
      module
      ;
  };
  config = import ./validate.nix { inherit lib system; } evaluated.config;
  derived = import ./derive.nix {
    inherit
      lib
      pkgs
      system
      constants
      config
      ;
  };
in
import ./emit-model.nix {
  inherit pkgs derived;
}
