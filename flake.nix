{
  description = "Nixfied framework (model-first architecture)";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      ...
    }:
    let
      supportedSystems = flake-utils.lib.defaultSystems;

      frameworkRevision = import ./nixfied/lib/framework-revision.nix {
        inherit self;
        sourcePath = ./.;
        metadataPath = ./nixfied/VENDORED.txt;
      };

      mkForSystem =
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          frameworkLib = import ./nixfied/lib {
            inherit
              pkgs
              system
              ;
          };

          compiled = frameworkLib.mkNixfied {
            projectRoot = ./.;
            projectModules = [ ./nixfied/project/module.nix ];
            extraModules = [ ];
            localOverrides = [ ];
            frameworkSourceRevision = frameworkRevision;
          };

          frameworkChecks = import ./tests/framework {
            inherit
              pkgs
              ;
            model = compiled.model;
            stateHash = compiled.stateHash;
            canonical = frameworkLib.canonical;
            registry = import ./nixfied/registry {
              inherit
                pkgs
                ;
              canonical = frameworkLib.canonical;
            };
          };
        in
        {
          apps = compiled.apps;
          packages = compiled.packages;
          checks = frameworkChecks;
          devShells = compiled.devShells;
        };
    in
    (flake-utils.lib.eachSystem supportedSystems mkForSystem)
    // {
      lib = {
        mkNixfied =
          {
            system,
            projectRoot,
            projectModules,
            extraModules ? [ ],
            localOverrides ? [ ],
            frameworkSourceRevision ? frameworkRevision,
          }:
          let
            pkgs = import nixpkgs { inherit system; };
            frameworkLib = import ./nixfied/lib {
              inherit
                pkgs
                system
                ;
            };
          in
          frameworkLib.mkNixfied {
            inherit
              projectRoot
              projectModules
              extraModules
              localOverrides
              frameworkSourceRevision
              ;
          };

        canonical = import ./nixfied/lib/canonical.nix { lib = nixpkgs.lib; };
      };

      nixfied = {
        modules = import ./nixfied/modules;
        schemas = {
          task = ./nixfied/schemas/task-contract.json;
          workflow = ./nixfied/schemas/workflow-contract.json;
          model = ./nixfied/schemas/model-export.json;
        };
      };
    };
}
