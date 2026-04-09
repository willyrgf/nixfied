{
  description = "Nixfied proof workspace seed";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    nixfied.url = "github:willyrgf/nixfied/dev";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    nixfied,
  }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        frameworkSourceRevision =
          let
            dirtyRev = if nixfied ? dirtyRev then nixfied.dirtyRev else null;
            rev = if nixfied ? rev then nixfied.rev else null;
            fallbackRevision = builtins.substring 0 12 (
              builtins.hashString "sha256" (builtins.toString nixfied.outPath)
            );
          in
          if dirtyRev != null then
            dirtyRev
          else if rev != null then
            rev
          else
            fallbackRevision;
        frameworkOutputs = nixfied.lib.mkFlakeOutputs {
          inherit system;
          projectRoot = ./.;
          projectModules = [ ./nixfied/project/module.nix ];
          extraModules = [ ];
          # Module overrides only. ./nixfied/local/default.nix is a legacy
          # extension file and is not loaded by the default flake outputs.
          localOverrides = [ ];
          inherit frameworkSourceRevision;
        };
      in {
        apps = frameworkOutputs.apps;
        packages = frameworkOutputs.packages;
        legacyPackages = frameworkOutputs.legacyPackages;
        checks = frameworkOutputs.checks;
        devShells = frameworkOutputs.devShells;
      });
}
