{
  description = "Nixfied proof workspace seed placeholder";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    # Bootstrap helpers rewrite this to the current source-repo path.
    nixfied.url = "path:/tmp/nixfied-source-not-materialized";
  };

  outputs =
    {
      flake-utils,
      nixfied,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        frameworkOutputs = nixfied.lib.mkFlakeOutputs {
          inherit system;
          projectRoot = ./.;
          projectModules = [ ./nixfied/project/module.nix ];
          extraModules = [ ];
          localOverrides = [ ];
          frameworkSourceRevision = "proof-seed";
        };
      in
      {
        inherit (frameworkOutputs)
          apps
          packages
          checks
          devShells
          legacyPackages
          ;
      }
    );
}
